//! The port of the yaml library's parse/parser.ts: the lexical tokens built into the concrete syntax tree.
//!
//! The library's handlers end in `yield* this.pop(); yield* this.step()` to hand the same token to the parent; that
//! recursion is as deep as the levels one token closes, which on a deeply nested text overflows a stack. Here a
//! handler returns `Flow::PopStep` instead and `step` loops, which is the same order of work. `onNewLine` is always
//! given (the line counter of `parseDocument`).

use super::cst::{Cst, DOCUMENT, FLOW_END, Id, Item, SCALAR, Src, Token, Ty, token_type};
use super::lexer::{Lexeme, lex};

/// what a handler leaves to do: nothing, or `yield* this.pop(); yield* this.step()`
enum Flow {
    Done,
    PopStep,
}

struct Parser {
    buf: Vec<u16>,
    tokens: Vec<Token>,
    top: Vec<(Id, usize)>,
    line_starts: Vec<usize>,
    at_new_line: bool,
    at_scalar: bool,
    indent: i64,
    offset: usize,
    on_key_line: bool,
    stack: Vec<Id>,
    source: Src,
    ty: Ty,
}

/// `new Parser(lineCounter.addNewLine).parse(text)`: every token of the text
pub(crate) fn parse(text: &str) -> Cst {
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    let len = buf.len();
    let lexemes = lex(&buf);
    buf.extend_from_slice(&[DOCUMENT, FLOW_END, SCALAR]);
    let mut p = Parser {
        buf,
        tokens: Vec::with_capacity(lexemes.len() + 8),
        top: Vec::new(),
        line_starts: vec![0], // `if (this.onNewLine && this.offset === 0) this.onNewLine(0)`
        at_new_line: true,
        at_scalar: false,
        indent: 0,
        offset: 0,
        on_key_line: false,
        stack: Vec::new(),
        source: Src::default(),
        ty: Ty::Newline,
    };
    for lexeme in lexemes {
        let source = match lexeme {
            Lexeme::Document => Src { start: len, end: len + 1 },
            Lexeme::FlowEnd => Src { start: len + 1, end: len + 2 },
            Lexeme::Scalar => Src { start: len + 2, end: len + 3 },
            Lexeme::Text(a, b) => Src { start: a, end: b },
        };
        p.next(source);
    }
    p.end();
    Cst { buf: p.buf, len, tokens: p.tokens, top: p.top, line_starts: p.line_starts }
}

fn is_flow_token(ty: Option<Ty>) -> bool {
    matches!(ty, Some(Ty::Alias | Ty::Scalar | Ty::SingleQuotedScalar | Ty::DoubleQuotedScalar | Ty::FlowCollection))
}

impl Parser {
    fn units(&self, s: Src) -> &[u16] {
        &self.buf[s.start..s.end]
    }
    fn ty_of(&self, id: Id) -> Ty {
        self.tokens[id].ty()
    }
    fn new_token(&mut self, t: Token) -> Id {
        self.tokens.push(t);
        self.tokens.len() - 1
    }
    /// `yield token` at the top level
    fn emit(&mut self, id: Id) {
        self.top.push((id, self.line_starts.len()));
    }
    fn on_new_line(&mut self, offset: usize) {
        self.line_starts.push(offset);
    }
    fn error_token(&mut self, message: String) -> Id {
        self.new_token(Token::Error { offset: self.offset, source: self.source, message })
    }
    /// `get sourceToken()`
    fn source_token(&mut self) -> Id {
        self.new_token(Token::Source { ty: self.ty, offset: self.offset, indent: self.indent, source: self.source })
    }
    fn peek(&self, n: usize) -> Option<Id> {
        self.stack.len().checked_sub(n).map(|i| self.stack[i])
    }
    /// `findNonEmptyIndex(list)`, None for -1
    fn find_non_empty_index(&self, list: &[Id]) -> Option<usize> {
        list.iter().position(|&t| !matches!(self.ty_of(t), Ty::Space | Ty::Comment | Ty::Newline))
    }
    fn includes(&self, list: &[Id], ty: Ty) -> bool {
        list.iter().any(|&t| self.ty_of(t) == ty)
    }
    fn items(&self, id: Id) -> &Vec<Item> {
        static NONE: Vec<Item> = Vec::new();
        self.tokens[id].items().unwrap_or(&NONE)
    }
    fn items_mut(&mut self, id: Id) -> &mut Vec<Item> {
        self.tokens[id].items_mut().expect("a collection token")
    }
    fn last_item(&self, id: Id) -> Option<&Item> {
        self.items(id).last()
    }
    fn last_item_mut(&mut self, id: Id) -> Option<&mut Item> {
        self.tokens[id].items_mut().and_then(|v| v.last_mut())
    }
    fn indent_of(&self, id: Id) -> i64 {
        self.tokens[id].indent().unwrap_or(0)
    }

    /// `getPrevProps(parent)` spliced by `getFirstKeyStartProps(prev)`: the props before a key, taken from where
    /// they were
    fn first_key_start_props_of_prev(&mut self, parent: Option<Id>) -> Vec<Id> {
        let Some(parent) = parent else { return Vec::new() };
        let list: Option<&Vec<Id>> = match &self.tokens[parent] {
            Token::Document { start, .. } => Some(start),
            Token::BlockMap { items, .. } => items.last().map(|it| it.sep.as_ref().unwrap_or(&it.start)),
            Token::BlockSeq { items, .. } => items.last().map(|it| &it.start),
            _ => None,
        };
        let Some(list) = list else { return Vec::new() };
        let at = self.first_key_start_index(list);
        let list = match &mut self.tokens[parent] {
            Token::Document { start, .. } => start,
            Token::BlockMap { items, .. } => match items.last_mut() {
                Some(it) => match it.sep.as_mut() {
                    Some(sep) => sep,
                    None => &mut it.start,
                },
                None => return Vec::new(),
            },
            Token::BlockSeq { items, .. } => match items.last_mut() {
                Some(it) => &mut it.start,
                None => return Vec::new(),
            },
            _ => return Vec::new(),
        };
        list.split_off(at.min(list.len()))
    }

    /// where `getFirstKeyStartProps(prev)` splices `prev`
    fn first_key_start_index(&self, prev: &[Id]) -> usize {
        if prev.is_empty() {
            return 0;
        }
        let mut i = prev.len() as isize;
        loop {
            i -= 1;
            if i < 0 {
                break;
            }
            if matches!(
                self.ty_of(prev[i as usize]),
                Ty::DocStart | Ty::ExplicitKeyInd | Ty::MapValueInd | Ty::SeqItemInd | Ty::Newline
            ) {
                break;
            }
        }
        loop {
            i += 1;
            match prev.get(i as usize) {
                Some(&t) if self.ty_of(t) == Ty::Space => {}
                _ => break,
            }
        }
        i as usize
    }

    /// `fixFlowSeqItems(fc)`
    fn fix_flow_seq_items(&mut self, fc: Id) {
        let Token::FlowCollection { start, items, .. } = &self.tokens[fc] else { return };
        if self.ty_of(*start) != Ty::FlowSeqStart {
            return;
        }
        for i in 0..items.len() {
            let it = &self.items(fc)[i];
            let Some(sep) = it.sep.as_ref() else { continue };
            if it.value.is_some() || self.includes(&it.start, Ty::ExplicitKeyInd) || self.includes(sep, Ty::MapValueInd)
            {
                continue;
            }
            let it = &mut self.items_mut(fc)[i];
            if let Some(key) = it.key.take() {
                it.value = Some(key);
            }
            let sep = it.sep.take().unwrap_or_default();
            let value = it.value;
            if is_flow_token(value.map(|v| self.ty_of(v))) {
                let Some(value) = value else { continue };
                match &mut self.tokens[value] {
                    Token::FlowScalar { end, .. } => match end {
                        Some(end) => end.extend(sep),
                        None => *end = Some(sep),
                    },
                    Token::FlowCollection { end, .. } => end.extend(sep),
                    _ => {}
                }
            } else {
                self.items_mut(fc)[i].start.extend(sep);
            }
        }
    }

    /// Advance the parser by the `source` of one lexical token.
    fn next(&mut self, source: Src) {
        self.source = source;
        let len = source.len();
        if self.at_scalar {
            self.at_scalar = false;
            self.step();
            self.offset += len;
            return;
        }
        let ty = token_type(self.units(source));
        match ty {
            None => {
                let message = format!("Not a YAML token: {}", String::from_utf16_lossy(self.units(source)));
                let e = self.error_token(message);
                self.pop(Some(e));
                self.offset += len;
            }
            Some(Ty::Scalar) => {
                self.at_new_line = false;
                self.at_scalar = true;
                self.ty = Ty::Scalar;
            }
            Some(ty) => {
                self.ty = ty;
                self.step();
                match ty {
                    Ty::Newline => {
                        self.at_new_line = true;
                        self.indent = 0;
                        self.on_new_line(self.offset + len);
                    }
                    Ty::Space => {
                        if self.at_new_line && self.units(source).first() == Some(&(b' ' as u16)) {
                            self.indent += len as i64;
                        }
                    }
                    Ty::ExplicitKeyInd | Ty::MapValueInd | Ty::SeqItemInd => {
                        if self.at_new_line {
                            self.indent += len as i64;
                        }
                    }
                    Ty::DocMode | Ty::FlowErrorEnd => return,
                    _ => self.at_new_line = false,
                }
                self.offset += len;
            }
        }
    }

    /// Call at end of input to push out any remaining constructions
    fn end(&mut self) {
        while !self.stack.is_empty() {
            self.pop(None);
        }
    }

    fn step(&mut self) {
        while let Flow::PopStep = self.step_once() {
            self.pop(None);
        }
    }

    fn step_once(&mut self) -> Flow {
        let top = self.peek(1);
        if self.ty == Ty::DocEnd && top.map(|t| self.ty_of(t)) != Some(Ty::DocEnd) {
            while !self.stack.is_empty() {
                self.pop(None);
            }
            let t = self.new_token(Token::DocEnd { offset: self.offset, source: self.source, end: None });
            self.stack.push(t);
            return Flow::Done;
        }
        let Some(top) = top else {
            self.stream();
            return Flow::Done;
        };
        match self.ty_of(top) {
            Ty::Document => self.document(top),
            Ty::Alias | Ty::Scalar | Ty::SingleQuotedScalar | Ty::DoubleQuotedScalar => self.scalar(top),
            Ty::BlockScalar => self.block_scalar(top),
            Ty::BlockMap => self.block_map(top),
            Ty::BlockSeq => self.block_sequence(top),
            Ty::FlowCollection => self.flow_collection(top),
            Ty::DocEnd => self.document_end(top),
            _ => {
                // should not happen
                self.pop(None);
                Flow::Done
            }
        }
    }

    fn pop(&mut self, error: Option<Id>) {
        let Some(token) = error.or_else(|| self.stack.pop()) else {
            let e = self.new_token(Token::Error {
                offset: self.offset,
                source: Src::default(),
                message: "Tried to pop an empty stack".to_string(),
            });
            self.emit(e);
            return;
        };
        loop {
            let Some(top) = self.peek(1) else {
                self.emit(token);
                return;
            };
            let token_ty = self.ty_of(token);
            let top_ty = self.ty_of(top);
            if token_ty == Ty::BlockScalar {
                // Block scalars use their parent rather than header indent
                let indent = self.tokens[top].indent().unwrap_or(0);
                self.tokens[token].set_indent(indent);
            } else if token_ty == Ty::FlowCollection && top_ty == Ty::Document {
                // Ignore all indent for top-level flow collections
                self.tokens[token].set_indent(0);
            }
            if token_ty == Ty::FlowCollection {
                self.fix_flow_seq_items(token);
            }
            match top_ty {
                Ty::Document => {
                    if let Token::Document { value, .. } = &mut self.tokens[top] {
                        *value = Some(token);
                    }
                }
                Ty::BlockScalar => {
                    // error
                    if let Token::BlockScalar { props, .. } = &mut self.tokens[top] {
                        props.push(token);
                    }
                }
                Ty::BlockMap => {
                    let (has_value, has_sep, explicit_key) = match self.last_item(top) {
                        Some(it) => (it.value.is_some(), it.sep.is_some(), it.explicit_key),
                        None => (false, false, false),
                    };
                    if has_value {
                        self.items_mut(top).push(Item {
                            key: Some(token),
                            sep: Some(Vec::new()),
                            ..Default::default()
                        });
                        self.on_key_line = true;
                        return;
                    } else if has_sep {
                        if let Some(it) = self.last_item_mut(top) {
                            it.value = Some(token);
                        }
                    } else {
                        if let Some(it) = self.last_item_mut(top) {
                            it.key = Some(token);
                            it.sep = Some(Vec::new());
                        }
                        self.on_key_line = !explicit_key;
                        return;
                    }
                }
                Ty::BlockSeq => match self.last_item_mut(top) {
                    Some(it) if it.value.is_none() => it.value = Some(token),
                    _ => self.items_mut(top).push(Item { value: Some(token), ..Default::default() }),
                },
                Ty::FlowCollection => {
                    match self.last_item_mut(top) {
                        Some(it) if it.value.is_none() => {
                            if it.sep.is_some() {
                                it.value = Some(token);
                            } else {
                                it.key = Some(token);
                                it.sep = Some(Vec::new());
                            }
                        }
                        _ => self.items_mut(top).push(Item {
                            key: Some(token),
                            sep: Some(Vec::new()),
                            ..Default::default()
                        }),
                    }
                    return;
                }
                _ => {
                    // should not happen: `yield* this.pop(); yield* this.pop(token)`
                    self.pop(None);
                    continue;
                }
            }
            if matches!(top_ty, Ty::Document | Ty::BlockMap | Ty::BlockSeq)
                && matches!(token_ty, Ty::BlockMap | Ty::BlockSeq)
            {
                let token_indent = self.indent_of(token);
                let moves = match self.last_item(token) {
                    Some(last) => {
                        last.sep.is_none()
                            && last.value.is_none()
                            && !last.start.is_empty()
                            && self.find_non_empty_index(&last.start).is_none()
                            && (token_indent == 0
                                || last
                                    .start
                                    .iter()
                                    .all(|&st| self.ty_of(st) != Ty::Comment || self.indent_of(st) < token_indent))
                    }
                    None => false,
                };
                if moves {
                    let start = self.items_mut(token).pop().map(|it| it.start).unwrap_or_default();
                    match &mut self.tokens[top] {
                        Token::Document { end, .. } => *end = Some(start),
                        Token::BlockMap { items, .. } | Token::BlockSeq { items, .. } => items.push(Item::start(start)),
                        _ => {}
                    }
                }
            }
            return;
        }
    }

    fn stream(&mut self) {
        match self.ty {
            Ty::DirectiveLine => {
                let t = self.new_token(Token::Directive { offset: self.offset, source: self.source });
                self.emit(t);
            }
            Ty::ByteOrderMark | Ty::Space | Ty::Comment | Ty::Newline => {
                let t = self.source_token();
                self.emit(t);
            }
            Ty::DocMode | Ty::DocStart => {
                let mut start = Vec::new();
                if self.ty == Ty::DocStart {
                    start.push(self.source_token());
                }
                let doc = self.new_token(Token::Document { offset: self.offset, start, value: None, end: None });
                self.stack.push(doc);
            }
            _ => {
                let e = self.error_token(format!("Unexpected {} token in YAML stream", self.ty.name()));
                self.emit(e);
            }
        }
    }

    fn document(&mut self, doc: Id) -> Flow {
        let Token::Document { start, value, .. } = &self.tokens[doc] else { return Flow::Done };
        if value.is_some() {
            return self.line_end(doc);
        }
        match self.ty {
            Ty::DocStart => {
                if self.find_non_empty_index(start).is_some() {
                    return Flow::PopStep;
                }
                let st = self.source_token();
                self.doc_start_push(doc, st);
                return Flow::Done;
            }
            Ty::Anchor | Ty::Tag | Ty::Space | Ty::Comment | Ty::Newline => {
                let st = self.source_token();
                self.doc_start_push(doc, st);
                return Flow::Done;
            }
            _ => {}
        }
        match self.start_block_value(Some(doc)) {
            Some(bv) => self.stack.push(bv),
            None => {
                let e = self.error_token(format!("Unexpected {} token in YAML document", self.ty.name()));
                self.emit(e);
            }
        }
        Flow::Done
    }

    fn doc_start_push(&mut self, doc: Id, st: Id) {
        if let Token::Document { start, .. } = &mut self.tokens[doc] {
            start.push(st);
        }
    }

    fn scalar(&mut self, scalar: Id) -> Flow {
        if self.ty == Ty::MapValueInd {
            let parent = self.peek(2);
            let start = self.first_key_start_props_of_prev(parent);
            let st = self.source_token();
            let (offset, indent, end) = match &mut self.tokens[scalar] {
                Token::FlowScalar { offset, indent, end, .. } => (*offset, *indent, end.take()),
                t => (t.offset(), t.indent().unwrap_or(0), None),
            };
            let sep = match end {
                Some(mut sep) => {
                    sep.push(st);
                    sep
                }
                None => vec![st],
            };
            let map = self.new_token(Token::BlockMap {
                offset,
                indent,
                items: vec![Item { start, key: Some(scalar), sep: Some(sep), ..Default::default() }],
            });
            self.on_key_line = true;
            if let Some(last) = self.stack.last_mut() {
                *last = map;
            }
            Flow::Done
        } else {
            self.line_end(scalar)
        }
    }

    fn block_scalar(&mut self, scalar: Id) -> Flow {
        match self.ty {
            Ty::Space | Ty::Comment | Ty::Newline => {
                let st = self.source_token();
                if let Token::BlockScalar { props, .. } = &mut self.tokens[scalar] {
                    props.push(st);
                }
                Flow::Done
            }
            Ty::Scalar => {
                let src = self.source;
                if let Token::BlockScalar { source, .. } = &mut self.tokens[scalar] {
                    *source = src;
                }
                // block-scalar source includes trailing newline
                self.at_new_line = true;
                self.indent = 0;
                self.count_new_lines_in_source();
                self.pop(None);
                Flow::Done
            }
            // should not happen
            _ => Flow::PopStep,
        }
    }

    /// `onNewLine` for every newline inside the current token's source
    fn count_new_lines_in_source(&mut self) {
        let (offset, src) = (self.offset, self.source);
        for k in src.start..src.end {
            if self.buf[k] == b'\n' as u16 {
                self.line_starts.push(offset + (k - src.start) + 1);
            }
        }
    }

    fn block_map(&mut self, map: Id) -> Flow {
        let Some(ii) = self.items(map).len().checked_sub(1) else { return Flow::PopStep };
        let map_indent = self.indent_of(map);
        // it.sep is true-ish if pair already has key or : separator
        match self.ty {
            Ty::Newline => {
                self.on_key_line = false;
                let st = self.source_token();
                let it = &self.items(map)[ii];
                if let Some(v) = it.value {
                    let last_is_comment =
                        self.tokens[v].end().and_then(|e| e.last()).is_some_and(|&l| self.ty_of(l) == Ty::Comment);
                    if last_is_comment {
                        self.push_end(v, st);
                    } else {
                        self.items_mut(map).push(Item::start(vec![st]));
                    }
                } else if it.sep.is_some() {
                    self.items_mut(map)[ii].sep.get_or_insert_with(Vec::new).push(st);
                } else {
                    self.items_mut(map)[ii].start.push(st);
                }
                return Flow::Done;
            }
            Ty::Space | Ty::Comment => {
                let st = self.source_token();
                let it = &self.items(map)[ii];
                if it.value.is_some() {
                    self.items_mut(map).push(Item::start(vec![st]));
                } else if it.sep.is_some() {
                    self.items_mut(map)[ii].sep.get_or_insert_with(Vec::new).push(st);
                } else {
                    if self.at_indented_comment(&it.start, map_indent) {
                        let prev_end = ii
                            .checked_sub(1)
                            .and_then(|p| self.items(map)[p].value)
                            .filter(|&v| self.tokens[v].end().is_some());
                        if let Some(v) = prev_end {
                            let start = std::mem::take(&mut self.items_mut(map)[ii].start);
                            for s in start {
                                self.push_end(v, s);
                            }
                            self.push_end(v, st);
                            self.items_mut(map).pop();
                            return Flow::Done;
                        }
                    }
                    self.items_mut(map)[ii].start.push(st);
                }
                return Flow::Done;
            }
            _ => {}
        }
        if self.indent >= map_indent {
            let it = &self.items(map)[ii];
            let at_map_indent = !self.on_key_line && self.indent == map_indent;
            let at_next_item = at_map_indent && (it.sep.is_some() || it.explicit_key) && self.ty != Ty::SeqItemInd;
            // For empty nodes, assign newline-separated not indented empty tokens to following node
            let mut start: Vec<Id> = Vec::new();
            if at_next_item
                && it.value.is_none()
                && let Some(sep) = it.sep.as_ref()
            {
                let mut nl: Vec<usize> = Vec::new();
                for (i, &st) in sep.iter().enumerate() {
                    match self.ty_of(st) {
                        Ty::Newline => nl.push(i),
                        Ty::Space => {}
                        Ty::Comment => {
                            if self.indent_of(st) > map_indent {
                                nl.clear();
                            }
                        }
                        _ => nl.clear(),
                    }
                }
                if nl.len() >= 2 {
                    let at = nl[1];
                    if let Some(sep) = self.items_mut(map)[ii].sep.as_mut() {
                        start = sep.split_off(at);
                    }
                }
            }
            let it = &self.items(map)[ii];
            let (has_value, has_sep, explicit_key) = (it.value.is_some(), it.sep.is_some(), it.explicit_key);
            match self.ty {
                Ty::Anchor | Ty::Tag => {
                    let st = self.source_token();
                    if at_next_item || has_value {
                        start.push(st);
                        self.items_mut(map).push(Item::start(start));
                        self.on_key_line = true;
                    } else if has_sep {
                        self.items_mut(map)[ii].sep.get_or_insert_with(Vec::new).push(st);
                    } else {
                        self.items_mut(map)[ii].start.push(st);
                    }
                    return Flow::Done;
                }
                Ty::ExplicitKeyInd => {
                    let st = self.source_token();
                    if !has_sep && !explicit_key {
                        let it = &mut self.items_mut(map)[ii];
                        it.start.push(st);
                        it.explicit_key = true;
                    } else if at_next_item || has_value {
                        start.push(st);
                        self.items_mut(map).push(Item { start, explicit_key: true, ..Default::default() });
                    } else {
                        let t = self.new_token(Token::BlockMap {
                            offset: self.offset,
                            indent: self.indent,
                            items: vec![Item { start: vec![st], explicit_key: true, ..Default::default() }],
                        });
                        self.stack.push(t);
                    }
                    self.on_key_line = true;
                    return Flow::Done;
                }
                Ty::MapValueInd => {
                    let st = self.source_token();
                    let sep_has = |p: &Self, ty: Ty| p.items(map)[ii].sep.as_ref().is_some_and(|s| p.includes(s, ty));
                    if explicit_key {
                        if !has_sep {
                            if self.includes(&self.items(map)[ii].start, Ty::Newline) {
                                let it = &mut self.items_mut(map)[ii];
                                it.key = None;
                                it.sep = Some(vec![st]);
                            } else {
                                let at = self.first_key_start_index(&self.items(map)[ii].start);
                                let start = self.items_mut(map)[ii].start.split_off(at);
                                let t = self.new_token(Token::BlockMap {
                                    offset: self.offset,
                                    indent: self.indent,
                                    items: vec![Item { start, sep: Some(vec![st]), ..Default::default() }],
                                });
                                self.stack.push(t);
                            }
                        } else if has_value {
                            self.items_mut(map).push(Item { sep: Some(vec![st]), ..Default::default() });
                        } else if sep_has(self, Ty::MapValueInd) {
                            let t = self.new_token(Token::BlockMap {
                                offset: self.offset,
                                indent: self.indent,
                                items: vec![Item { start, sep: Some(vec![st]), ..Default::default() }],
                            });
                            self.stack.push(t);
                        } else if is_flow_token(self.items(map)[ii].key.map(|k| self.ty_of(k)))
                            && !sep_has(self, Ty::Newline)
                        {
                            let at = self.first_key_start_index(&self.items(map)[ii].start);
                            let it = &mut self.items_mut(map)[ii];
                            let start = it.start.split_off(at);
                            let key = it.key.take();
                            let mut sep = it.sep.take().unwrap_or_default();
                            sep.push(st);
                            let t = self.new_token(Token::BlockMap {
                                offset: self.offset,
                                indent: self.indent,
                                items: vec![Item { start, key, sep: Some(sep), ..Default::default() }],
                            });
                            self.stack.push(t);
                        } else if !start.is_empty() {
                            // Not actually at next item
                            let sep = self.items_mut(map)[ii].sep.get_or_insert_with(Vec::new);
                            sep.extend(start);
                            sep.push(st);
                        } else {
                            self.items_mut(map)[ii].sep.get_or_insert_with(Vec::new).push(st);
                        }
                    } else if !has_sep {
                        let it = &mut self.items_mut(map)[ii];
                        it.key = None;
                        it.sep = Some(vec![st]);
                    } else if has_value || at_next_item {
                        self.items_mut(map).push(Item { start, sep: Some(vec![st]), ..Default::default() });
                    } else if sep_has(self, Ty::MapValueInd) {
                        let t = self.new_token(Token::BlockMap {
                            offset: self.offset,
                            indent: self.indent,
                            items: vec![Item { sep: Some(vec![st]), ..Default::default() }],
                        });
                        self.stack.push(t);
                    } else {
                        self.items_mut(map)[ii].sep.get_or_insert_with(Vec::new).push(st);
                    }
                    self.on_key_line = true;
                    return Flow::Done;
                }
                Ty::Alias | Ty::Scalar | Ty::SingleQuotedScalar | Ty::DoubleQuotedScalar => {
                    let fs = self.flow_scalar(self.ty);
                    if at_next_item || has_value {
                        self.items_mut(map).push(Item {
                            start,
                            key: Some(fs),
                            sep: Some(Vec::new()),
                            ..Default::default()
                        });
                        self.on_key_line = true;
                    } else if has_sep {
                        self.stack.push(fs);
                    } else {
                        let it = &mut self.items_mut(map)[ii];
                        it.key = Some(fs);
                        it.sep = Some(Vec::new());
                        self.on_key_line = true;
                    }
                    return Flow::Done;
                }
                _ => {
                    if let Some(bv) = self.start_block_value(Some(map)) {
                        if self.ty_of(bv) == Ty::BlockSeq {
                            let it = &self.items(map)[ii];
                            if !it.explicit_key && it.sep.as_ref().is_some_and(|s| !self.includes(s, Ty::Newline)) {
                                let e = self.error_token("Unexpected block-seq-ind on same line with key".to_string());
                                self.pop(Some(e));
                                return Flow::Done;
                            }
                        } else if at_map_indent {
                            self.items_mut(map).push(Item::start(start));
                        }
                        self.stack.push(bv);
                        return Flow::Done;
                    }
                }
            }
        }
        Flow::PopStep
    }

    /// `token.end.push(st)` on a token that has an end
    fn push_end(&mut self, id: Id, st: Id) {
        match &mut self.tokens[id] {
            Token::FlowScalar { end, .. } | Token::Document { end, .. } | Token::DocEnd { end, .. } => {
                end.get_or_insert_with(Vec::new).push(st)
            }
            Token::FlowCollection { end, .. } => end.push(st),
            _ => {}
        }
    }

    fn block_sequence(&mut self, seq: Id) -> Flow {
        let Some(ii) = self.items(seq).len().checked_sub(1) else { return Flow::PopStep };
        let seq_indent = self.indent_of(seq);
        match self.ty {
            Ty::Newline => {
                let st = self.source_token();
                if let Some(v) = self.items(seq)[ii].value {
                    let last_is_comment =
                        self.tokens[v].end().and_then(|e| e.last()).is_some_and(|&l| self.ty_of(l) == Ty::Comment);
                    if last_is_comment {
                        self.push_end(v, st);
                    } else {
                        self.items_mut(seq).push(Item::start(vec![st]));
                    }
                } else {
                    self.items_mut(seq)[ii].start.push(st);
                }
                return Flow::Done;
            }
            Ty::Space | Ty::Comment => {
                let st = self.source_token();
                if self.items(seq)[ii].value.is_some() {
                    self.items_mut(seq).push(Item::start(vec![st]));
                } else {
                    if self.at_indented_comment(&self.items(seq)[ii].start, seq_indent) {
                        let prev_end = ii
                            .checked_sub(1)
                            .and_then(|p| self.items(seq)[p].value)
                            .filter(|&v| self.tokens[v].end().is_some());
                        if let Some(v) = prev_end {
                            let start = std::mem::take(&mut self.items_mut(seq)[ii].start);
                            for s in start {
                                self.push_end(v, s);
                            }
                            self.push_end(v, st);
                            self.items_mut(seq).pop();
                            return Flow::Done;
                        }
                    }
                    self.items_mut(seq)[ii].start.push(st);
                }
                return Flow::Done;
            }
            Ty::Anchor | Ty::Tag => {
                if !(self.items(seq)[ii].value.is_some() || self.indent <= seq_indent) {
                    let st = self.source_token();
                    self.items_mut(seq)[ii].start.push(st);
                    return Flow::Done;
                }
            }
            Ty::SeqItemInd if self.indent == seq_indent => {
                let st = self.source_token();
                let it = &self.items(seq)[ii];
                if it.value.is_some() || self.includes(&it.start, Ty::SeqItemInd) {
                    self.items_mut(seq).push(Item::start(vec![st]));
                } else {
                    self.items_mut(seq)[ii].start.push(st);
                }
                return Flow::Done;
            }
            _ => {}
        }
        if self.indent > seq_indent
            && let Some(bv) = self.start_block_value(Some(seq))
        {
            self.stack.push(bv);
            return Flow::Done;
        }
        Flow::PopStep
    }

    fn flow_collection(&mut self, fc: Id) -> Flow {
        let Token::FlowCollection { end: fc_end, indent: fc_indent, offset: fc_offset, .. } = &self.tokens[fc] else {
            return Flow::Done;
        };
        let (fc_end_empty, fc_indent, fc_offset) = (fc_end.is_empty(), *fc_indent, *fc_offset);
        let last = self.items(fc).len().checked_sub(1);
        if self.ty == Ty::FlowErrorEnd {
            loop {
                self.pop(None);
                match self.peek(1) {
                    Some(top) if self.ty_of(top) == Ty::FlowCollection => {}
                    _ => break,
                }
            }
            return Flow::Done;
        }
        if fc_end_empty {
            let it = last.map(|i| &self.items(fc)[i]);
            let (exists, has_sep, has_value) = match it {
                Some(it) => (true, it.sep.is_some(), it.value.is_some()),
                None => (false, false, false),
            };
            match self.ty {
                Ty::Comma | Ty::ExplicitKeyInd => {
                    let st = self.source_token();
                    match last {
                        Some(i) if !has_sep => self.items_mut(fc)[i].start.push(st),
                        _ => self.items_mut(fc).push(Item::start(vec![st])),
                    }
                    return Flow::Done;
                }
                Ty::MapValueInd => {
                    let st = self.source_token();
                    match last {
                        Some(i) if !has_value => {
                            let it = &mut self.items_mut(fc)[i];
                            match it.sep.as_mut() {
                                Some(sep) => sep.push(st),
                                None => {
                                    it.key = None;
                                    it.sep = Some(vec![st]);
                                }
                            }
                        }
                        _ => self.items_mut(fc).push(Item { sep: Some(vec![st]), ..Default::default() }),
                    }
                    return Flow::Done;
                }
                Ty::Space | Ty::Comment | Ty::Newline | Ty::Anchor | Ty::Tag => {
                    let st = self.source_token();
                    match last {
                        Some(i) if !has_value => {
                            let it = &mut self.items_mut(fc)[i];
                            match it.sep.as_mut() {
                                Some(sep) => sep.push(st),
                                None => it.start.push(st),
                            }
                        }
                        _ => self.items_mut(fc).push(Item::start(vec![st])),
                    }
                    return Flow::Done;
                }
                Ty::Alias | Ty::Scalar | Ty::SingleQuotedScalar | Ty::DoubleQuotedScalar => {
                    let fs = self.flow_scalar(self.ty);
                    if !exists || has_value {
                        self.items_mut(fc).push(Item { key: Some(fs), sep: Some(Vec::new()), ..Default::default() });
                    } else if has_sep {
                        self.stack.push(fs);
                    } else if let Some(i) = last {
                        let it = &mut self.items_mut(fc)[i];
                        it.key = Some(fs);
                        it.sep = Some(Vec::new());
                    }
                    return Flow::Done;
                }
                Ty::FlowMapEnd | Ty::FlowSeqEnd => {
                    let st = self.source_token();
                    self.push_end(fc, st);
                    return Flow::Done;
                }
                _ => {}
            }
            match self.start_block_value(Some(fc)) {
                Some(bv) => {
                    self.stack.push(bv);
                    Flow::Done
                }
                None => Flow::PopStep,
            }
        } else {
            let parent = self.peek(2);
            let parent_ty = parent.map(|p| self.ty_of(p));
            let parent_block_map_ends = match parent {
                Some(p) if parent_ty == Some(Ty::BlockMap) => {
                    (self.ty == Ty::MapValueInd && self.indent_of(p) == fc_indent)
                        || (self.ty == Ty::Newline && self.last_item(p).is_some_and(|it| it.sep.is_none()))
                }
                _ => false,
            };
            if parent_block_map_ends {
                Flow::PopStep
            } else if self.ty == Ty::MapValueInd && parent.is_some() && parent_ty != Some(Ty::FlowCollection) {
                let start = self.first_key_start_props_of_prev(parent);
                self.fix_flow_seq_items(fc);
                let st = self.source_token();
                let mut sep = match &mut self.tokens[fc] {
                    Token::FlowCollection { end, .. } => end.split_off(1.min(end.len())),
                    _ => Vec::new(),
                };
                sep.push(st);
                let map = self.new_token(Token::BlockMap {
                    offset: fc_offset,
                    indent: fc_indent,
                    items: vec![Item { start, key: Some(fc), sep: Some(sep), ..Default::default() }],
                });
                self.on_key_line = true;
                if let Some(last) = self.stack.last_mut() {
                    *last = map;
                }
                Flow::Done
            } else {
                self.line_end(fc)
            }
        }
    }

    fn flow_scalar(&mut self, ty: Ty) -> Id {
        self.count_new_lines_in_source();
        self.new_token(Token::FlowScalar {
            ty,
            offset: self.offset,
            indent: self.indent,
            source: self.source,
            end: None,
        })
    }

    fn start_block_value(&mut self, parent: Option<Id>) -> Option<Id> {
        match self.ty {
            Ty::Alias | Ty::Scalar | Ty::SingleQuotedScalar | Ty::DoubleQuotedScalar => Some(self.flow_scalar(self.ty)),
            Ty::BlockScalarHeader => {
                let st = self.source_token();
                Some(self.new_token(Token::BlockScalar {
                    offset: self.offset,
                    indent: self.indent,
                    props: vec![st],
                    source: Src::default(),
                }))
            }
            Ty::FlowMapStart | Ty::FlowSeqStart => {
                let st = self.source_token();
                Some(self.new_token(Token::FlowCollection {
                    offset: self.offset,
                    indent: self.indent,
                    start: st,
                    items: Vec::new(),
                    end: Vec::new(),
                }))
            }
            Ty::SeqItemInd => {
                let st = self.source_token();
                Some(self.new_token(Token::BlockSeq {
                    offset: self.offset,
                    indent: self.indent,
                    items: vec![Item::start(vec![st])],
                }))
            }
            Ty::ExplicitKeyInd => {
                self.on_key_line = true;
                let mut start = self.first_key_start_props_of_prev(parent);
                start.push(self.source_token());
                Some(self.new_token(Token::BlockMap {
                    offset: self.offset,
                    indent: self.indent,
                    items: vec![Item { start, explicit_key: true, ..Default::default() }],
                }))
            }
            Ty::MapValueInd => {
                self.on_key_line = true;
                let start = self.first_key_start_props_of_prev(parent);
                let st = self.source_token();
                Some(self.new_token(Token::BlockMap {
                    offset: self.offset,
                    indent: self.indent,
                    items: vec![Item { start, sep: Some(vec![st]), ..Default::default() }],
                }))
            }
            _ => None,
        }
    }

    fn at_indented_comment(&self, start: &[Id], indent: i64) -> bool {
        if self.ty != Ty::Comment {
            return false;
        }
        if self.indent <= indent {
            return false;
        }
        start.iter().all(|&st| matches!(self.ty_of(st), Ty::Newline | Ty::Space))
    }

    fn document_end(&mut self, doc_end: Id) -> Flow {
        if self.ty != Ty::DocMode {
            let st = self.source_token();
            self.push_end(doc_end, st);
            if self.ty == Ty::Newline {
                self.pop(None);
            }
        }
        Flow::Done
    }

    fn line_end(&mut self, token: Id) -> Flow {
        match self.ty {
            Ty::Comma | Ty::DocStart | Ty::DocEnd | Ty::FlowSeqEnd | Ty::FlowMapEnd | Ty::MapValueInd => Flow::PopStep,
            _ => {
                if self.ty == Ty::Newline {
                    self.on_key_line = false;
                }
                // all other values are errors
                let st = self.source_token();
                self.push_end(token, st);
                if self.ty == Ty::Newline {
                    self.pop(None);
                }
                Flow::Done
            }
        }
    }
}
