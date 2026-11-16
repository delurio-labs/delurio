//! The concrete syntax tree: the port of the yaml library's parse/cst.ts (`tokenType`, the control characters) and
//! of the token shapes of parse/cst.d.ts.
//!
//! The library's tokens are JavaScript objects that refer to each other; here they live in one arena (`Cst::tokens`)
//! and refer to each other by index, so that a deeply nested text is neither built, walked nor dropped by recursion.
//! A token's `source` is a range of `Cst::buf`: the text in UTF-16 code units (every offset of the library counts
//! them), followed by the three control characters the lexer emits, so that a token whose source is one of them
//! has, as in JavaScript, a source of length 1.

/// an index into `Cst::tokens`
pub(crate) type Id = usize;

/// `BOM`, `DOCUMENT`, `FLOW_END` and `SCALAR` of cst.ts
pub(crate) const BOM: u16 = 0xFEFF;
pub(crate) const DOCUMENT: u16 = 0x02;
pub(crate) const FLOW_END: u16 = 0x18;
pub(crate) const SCALAR: u16 = 0x1f;

/// a token's `type`: the lexical types of `tokenType`, and the types of the tree's own tokens
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ty {
    ByteOrderMark,
    DocMode,
    FlowErrorEnd,
    Scalar,
    DocStart,
    DocEnd,
    Newline,
    SeqItemInd,
    ExplicitKeyInd,
    MapValueInd,
    FlowMapStart,
    FlowMapEnd,
    FlowSeqStart,
    FlowSeqEnd,
    Comma,
    Space,
    Comment,
    DirectiveLine,
    Alias,
    Anchor,
    Tag,
    SingleQuotedScalar,
    DoubleQuotedScalar,
    BlockScalarHeader,
    // the tree's own
    Directive,
    Document,
    BlockScalar,
    BlockMap,
    BlockSeq,
    FlowCollection,
    Error,
}

impl Ty {
    /// the type's name, as the library's messages say it
    pub(crate) fn name(self) -> &'static str {
        match self {
            Ty::ByteOrderMark => "byte-order-mark",
            Ty::DocMode => "doc-mode",
            Ty::FlowErrorEnd => "flow-error-end",
            Ty::Scalar => "scalar",
            Ty::DocStart => "doc-start",
            Ty::DocEnd => "doc-end",
            Ty::Newline => "newline",
            Ty::SeqItemInd => "seq-item-ind",
            Ty::ExplicitKeyInd => "explicit-key-ind",
            Ty::MapValueInd => "map-value-ind",
            Ty::FlowMapStart => "flow-map-start",
            Ty::FlowMapEnd => "flow-map-end",
            Ty::FlowSeqStart => "flow-seq-start",
            Ty::FlowSeqEnd => "flow-seq-end",
            Ty::Comma => "comma",
            Ty::Space => "space",
            Ty::Comment => "comment",
            Ty::DirectiveLine => "directive-line",
            Ty::Alias => "alias",
            Ty::Anchor => "anchor",
            Ty::Tag => "tag",
            Ty::SingleQuotedScalar => "single-quoted-scalar",
            Ty::DoubleQuotedScalar => "double-quoted-scalar",
            Ty::BlockScalarHeader => "block-scalar-header",
            Ty::Directive => "directive",
            Ty::Document => "document",
            Ty::BlockScalar => "block-scalar",
            Ty::BlockMap => "block-map",
            Ty::BlockSeq => "block-seq",
            Ty::FlowCollection => "flow-collection",
            Ty::Error => "error",
        }
    }
}

/// `tokenType(source)`: the type of a lexical token, None for one that is not a YAML token
pub(crate) fn token_type(s: &[u16]) -> Option<Ty> {
    const fn b(c: u8) -> u16 {
        c as u16
    }
    let whole = match s {
        [BOM] => Some(Ty::ByteOrderMark),
        [DOCUMENT] => Some(Ty::DocMode),
        [FLOW_END] => Some(Ty::FlowErrorEnd),
        [SCALAR] => Some(Ty::Scalar),
        [a, b2, c] if *a == b(b'-') && *b2 == b(b'-') && *c == b(b'-') => Some(Ty::DocStart),
        [a, b2, c] if *a == b(b'.') && *b2 == b(b'.') && *c == b(b'.') => Some(Ty::DocEnd),
        [] => Some(Ty::Newline),
        [a] if *a == b(b'\n') => Some(Ty::Newline),
        [a, b2] if *a == b(b'\r') && *b2 == b(b'\n') => Some(Ty::Newline),
        [a] => match *a {
            0x2d => Some(Ty::SeqItemInd),     // -
            0x3f => Some(Ty::ExplicitKeyInd), // ?
            0x3a => Some(Ty::MapValueInd),    // :
            0x7b => Some(Ty::FlowMapStart),   // {
            0x7d => Some(Ty::FlowMapEnd),     // }
            0x5b => Some(Ty::FlowSeqStart),   // [
            0x5d => Some(Ty::FlowSeqEnd),     // ]
            0x2c => Some(Ty::Comma),          // ,
            _ => None,
        },
        _ => None,
    };
    if whole.is_some() {
        return whole;
    }
    match s.first().copied()? {
        0x20 | 0x09 => Some(Ty::Space),
        0x23 => Some(Ty::Comment),
        0x25 => Some(Ty::DirectiveLine),
        0x2a => Some(Ty::Alias),
        0x26 => Some(Ty::Anchor),
        0x21 => Some(Ty::Tag),
        0x27 => Some(Ty::SingleQuotedScalar),
        0x22 => Some(Ty::DoubleQuotedScalar),
        0x7c | 0x3e => Some(Ty::BlockScalarHeader),
        _ => None,
    }
}

/// a token's `source`: a range of `Cst::buf`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Src {
    pub start: usize,
    pub end: usize,
}

impl Src {
    pub(crate) fn len(self) -> usize {
        self.end - self.start
    }
    pub(crate) fn is_empty(self) -> bool {
        self.end == self.start
    }
}

/// an item of a collection: `{ start, key?, sep?, value?, explicitKey? }`. `key` is None both where the library has no
/// key and where it has `key: null`, which no reader tells apart.
#[derive(Debug, Clone, Default)]
pub(crate) struct Item {
    pub start: Vec<Id>,
    pub key: Option<Id>,
    pub sep: Option<Vec<Id>>,
    pub value: Option<Id>,
    pub explicit_key: bool,
}

impl Item {
    pub(crate) fn start(start: Vec<Id>) -> Self {
        Item { start, ..Default::default() }
    }
}

/// a token of the tree
#[derive(Debug, Clone)]
pub(crate) enum Token {
    /// a lexical token as the parser keeps it: `SourceToken`
    Source {
        ty: Ty,
        offset: usize,
        indent: i64,
        source: Src,
    },
    Directive {
        offset: usize,
        source: Src,
    },
    Error {
        offset: usize,
        source: Src,
        message: String,
    },
    DocEnd {
        offset: usize,
        source: Src,
        end: Option<Vec<Id>>,
    },
    Document {
        offset: usize,
        start: Vec<Id>,
        value: Option<Id>,
        end: Option<Vec<Id>>,
    },
    /// `FlowScalar`: its type is alias, scalar, single-quoted-scalar or double-quoted-scalar
    FlowScalar {
        ty: Ty,
        offset: usize,
        indent: i64,
        source: Src,
        end: Option<Vec<Id>>,
    },
    BlockScalar {
        offset: usize,
        indent: i64,
        props: Vec<Id>,
        source: Src,
    },
    BlockMap {
        offset: usize,
        indent: i64,
        items: Vec<Item>,
    },
    BlockSeq {
        offset: usize,
        indent: i64,
        items: Vec<Item>,
    },
    FlowCollection {
        offset: usize,
        indent: i64,
        start: Id,
        items: Vec<Item>,
        end: Vec<Id>,
    },
}

impl Token {
    pub(crate) fn ty(&self) -> Ty {
        match self {
            Token::Source { ty, .. } | Token::FlowScalar { ty, .. } => *ty,
            Token::Directive { .. } => Ty::Directive,
            Token::Error { .. } => Ty::Error,
            Token::DocEnd { .. } => Ty::DocEnd,
            Token::Document { .. } => Ty::Document,
            Token::BlockScalar { .. } => Ty::BlockScalar,
            Token::BlockMap { .. } => Ty::BlockMap,
            Token::BlockSeq { .. } => Ty::BlockSeq,
            Token::FlowCollection { .. } => Ty::FlowCollection,
        }
    }
    pub(crate) fn offset(&self) -> usize {
        match self {
            Token::Source { offset, .. }
            | Token::Directive { offset, .. }
            | Token::Error { offset, .. }
            | Token::DocEnd { offset, .. }
            | Token::Document { offset, .. }
            | Token::FlowScalar { offset, .. }
            | Token::BlockScalar { offset, .. }
            | Token::BlockMap { offset, .. }
            | Token::BlockSeq { offset, .. }
            | Token::FlowCollection { offset, .. } => *offset,
        }
    }
    /// `token.indent`, None where the token has none (`'indent' in token` is false)
    pub(crate) fn indent(&self) -> Option<i64> {
        match self {
            Token::Source { indent, .. }
            | Token::FlowScalar { indent, .. }
            | Token::BlockScalar { indent, .. }
            | Token::BlockMap { indent, .. }
            | Token::BlockSeq { indent, .. }
            | Token::FlowCollection { indent, .. } => Some(*indent),
            _ => None,
        }
    }
    pub(crate) fn set_indent(&mut self, to: i64) {
        match self {
            Token::Source { indent, .. }
            | Token::FlowScalar { indent, .. }
            | Token::BlockScalar { indent, .. }
            | Token::BlockMap { indent, .. }
            | Token::BlockSeq { indent, .. }
            | Token::FlowCollection { indent, .. } => *indent = to,
            _ => {}
        }
    }
    /// `token.source`, None where the token has none (a collection, a document)
    pub(crate) fn source(&self) -> Option<Src> {
        match self {
            Token::Source { source, .. }
            | Token::Directive { source, .. }
            | Token::Error { source, .. }
            | Token::DocEnd { source, .. }
            | Token::FlowScalar { source, .. }
            | Token::BlockScalar { source, .. } => Some(*source),
            _ => None,
        }
    }
    /// `token.end` where it is set: a flow scalar's once the parser gave it one, a flow collection's always
    pub(crate) fn end(&self) -> Option<&Vec<Id>> {
        match self {
            Token::FlowScalar { end, .. } | Token::Document { end, .. } | Token::DocEnd { end, .. } => end.as_ref(),
            Token::FlowCollection { end, .. } => Some(end),
            _ => None,
        }
    }
    pub(crate) fn items(&self) -> Option<&Vec<Item>> {
        match self {
            Token::BlockMap { items, .. } | Token::BlockSeq { items, .. } | Token::FlowCollection { items, .. } => {
                Some(items)
            }
            _ => None,
        }
    }
    pub(crate) fn items_mut(&mut self) -> Option<&mut Vec<Item>> {
        match self {
            Token::BlockMap { items, .. } | Token::BlockSeq { items, .. } | Token::FlowCollection { items, .. } => {
                Some(items)
            }
            _ => None,
        }
    }
}

/// what the parser made of a text: its tokens, the top-level ones in the order the parser yielded them (each with how
/// many line starts the line counter had by then), and the line starts of the whole text
#[derive(Debug)]
pub(crate) struct Cst {
    /// the text in UTF-16 code units, then DOCUMENT, FLOW_END and SCALAR
    pub buf: Vec<u16>,
    /// the text's length, in UTF-16 code units
    pub len: usize,
    pub tokens: Vec<Token>,
    pub top: Vec<(Id, usize)>,
    pub line_starts: Vec<usize>,
}

impl Cst {
    pub(crate) fn tok(&self, id: Id) -> &Token {
        &self.tokens[id]
    }
    pub(crate) fn ty(&self, id: Id) -> Ty {
        self.tokens[id].ty()
    }
    /// a token's source as UTF-16 code units
    pub(crate) fn units(&self, s: Src) -> &[u16] {
        self.buf.get(s.start..s.end).unwrap_or(&[])
    }
    /// a token's source as a string
    pub(crate) fn text(&self, s: Src) -> String {
        String::from_utf16_lossy(self.units(s))
    }
    /// the source of a token, empty where it has none
    pub(crate) fn source_of(&self, id: Id) -> &[u16] {
        self.tokens[id].source().map_or(&[], |s| self.units(s))
    }
}
