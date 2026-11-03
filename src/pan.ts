// The diagram as a canvas to move around, as in a design tool: it opens fitted to its box, a drag moves it (the hand),
// Ctrl or ⌘ with the wheel, or a pinch, zooms at the pointer, and the wheel alone moves it until an edge, after which
// the page scrolls on. Shift+1 fits it again, Shift+0 shows it at 100%, and "Full window" gives it the whole window.
// Without scripts the page keeps its plain diagram, which scrolls sideways.

export const PAN_CSS = `
.pz-on { position: relative; height: 78vh; min-height: 360px; overflow: hidden; margin-left: calc(50% - 50vw + 24px); margin-right: calc(50% - 50vw + 24px); cursor: grab; touch-action: none; user-select: none; -webkit-user-select: none; }
.pz-on.pz-grabbing, .pz-on.pz-grabbing a { cursor: grabbing; }
.pz-on:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.pz-on svg.spine { width: 100%; height: 100%; max-width: none; padding: 0; margin: 0; }
.pz-on a { cursor: pointer; -webkit-user-drag: none; }
.pz-bar { position: absolute; top: 10px; right: 10px; display: flex; gap: 4px; cursor: default; }
.pz-bar button { font: 13px system-ui, sans-serif; min-width: 32px; height: 30px; padding: 0 10px; border: 1px solid var(--line); border-radius: 8px; background: var(--surface); color: var(--ink); cursor: pointer; }
.pz-bar button:hover { border-color: var(--muted); } .pz-bar button:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
.pz-bar .pz-pct { min-width: 58px; font-variant-numeric: tabular-nums; }
.pz-hint { position: absolute; left: 10px; bottom: 10px; margin: 0; padding: 2px 8px; border-radius: 6px; background: var(--surface); color: var(--muted); font-size: 12px; pointer-events: none; }
html.pz-lock, html.pz-lock body { overflow: hidden; }
.pz-host.pz-full { position: fixed; inset: 0; z-index: 100; margin: 0; padding: 10px 12px 12px; background: var(--bg); display: grid; grid-template-columns: auto 1fr; grid-template-rows: auto minmax(0, 1fr); align-items: center; }
.pz-host.pz-full { row-gap: 10px; } .pz-host.pz-full > label { margin: 0; }
.pz-host.pz-full > .pz-on { grid-column: 1 / -1; grid-row: 2; align-self: stretch; height: auto; min-height: 0; margin: 0; }
.pz-on.pz-full { position: fixed; inset: 0; z-index: 100; height: auto; min-height: 0; margin: 0; border-radius: 0; }
`;

// plain JavaScript, run as the page loads; it reads no data and sends nothing
export const PAN_SCRIPT = `<script>(() => {
  const PAD = 24, MAX = 4;
  for (const box of document.querySelectorAll('.pz')) {
    const svg = box.querySelector('svg.spine');
    if (!svg || !svg.viewBox.baseVal || !svg.viewBox.baseVal.width) continue;
    const W = svg.viewBox.baseVal.width, H = svg.viewBox.baseVal.height;
    const host = box.closest('.pz-host') || box;
    svg.removeAttribute('style');
    box.classList.add('pz-on');
    box.tabIndex = 0;

    let s = 1, x = 0, y = 0, vw = 0, vh = 0, fitted = true;
    const bar = document.createElement('div');
    bar.className = 'pz-bar';
    const button = (text, title, act) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = text;
      b.title = title;
      b.addEventListener('click', act);
      bar.append(b);
      return b;
    };
    const fitScale = () => Math.max(Math.min((vw - 2 * PAD) / W, (vh - 2 * PAD) / H), 0.001);
    const minScale = () => Math.min(fitScale(), 1) / 2;
    // where the box's top left may be on an axis, in the diagram's units. Snug: centred when the diagram fits on it,
    // else from PAD before its start to PAD past its end. The hand may go further, until the diagram's edge reaches
    // the middle of the box, so it is never lost.
    const snug = (len, view) => {
      const p = PAD / s;
      return len + 2 * p <= view ? [(len - view) / 2, (len - view) / 2] : [-p, len + p - view];
    };
    const free = (len, view) => {
      const [lo, hi] = snug(len, view);
      return [Math.min(lo, -view / 2), Math.max(hi, len - view / 2)];
    };
    const within = (v, [lo, hi]) => Math.min(Math.max(v, lo), hi);
    const clamp = () => {
      x = within(x, free(W, vw / s));
      y = within(y, free(H, vh / s));
    };
    const draw = () => {
      clamp();
      svg.setAttribute('viewBox', x + ' ' + y + ' ' + vw / s + ' ' + vh / s);
      pct.textContent = Math.round(s * 100) + '%';
    };
    const zoomAt = (px, py, factor) => {
      const ns = Math.min(MAX, Math.max(minScale(), s * factor));
      const dx = x + px / s, dy = y + py / s;
      s = ns;
      x = dx - px / s;
      y = dy - py / s;
      fitted = false;
      draw();
    };
    const fit = () => {
      s = fitScale();
      x = (W - vw / s) / 2;
      y = (H - vh / s) / 2;
      fitted = true;
      draw();
    };
    const actual = () => zoomAt(vw / 2, vh / 2, 1 / s);
    const setFull = (on) => {
      host.classList.toggle('pz-full', on);
      document.documentElement.classList.toggle('pz-lock', on);
      for (const b of host.querySelectorAll('.pz-fs')) b.textContent = on ? 'Exit full window' : 'Full window';
      if (on) box.focus({ preventScroll: true });
    };

    button('\\u2212', 'Zoom out (\\u2212)', () => zoomAt(vw / 2, vh / 2, 0.8));
    const pct = button('100%', 'Show at 100% (Shift+0)', actual);
    pct.className = 'pz-pct';
    button('+', 'Zoom in (+)', () => zoomAt(vw / 2, vh / 2, 1.25));
    button('Fit', 'Fit the whole diagram (Shift+1)', fit);
    button('Full window', 'Give the diagram the whole window (F; Esc leaves it)', () => setFull(!host.classList.contains('pz-full'))).className = 'pz-fs';
    const hint = document.createElement('p');
    hint.className = 'pz-hint';
    hint.textContent = 'Drag to move \\u00b7 Ctrl or \\u2318 + scroll, or pinch, to zoom \\u00b7 Shift+1 fits';
    box.append(bar, hint);

    // a new size keeps a fitted diagram fitted, and keeps the centre of one zoomed by hand; a hidden box (the other
    // view) keeps its place for when it shows again
    new ResizeObserver(() => {
      const nw = box.clientWidth, nh = box.clientHeight;
      if (!nw || !nh) return;
      const first = !vw, cx = x + vw / (2 * s), cy = y + vh / (2 * s);
      vw = nw;
      vh = nh;
      if (first || fitted) return fit();
      x = cx - vw / (2 * s);
      y = cy - vh / (2 * s);
      draw();
    }).observe(box);

    let gesture = false;
    box.addEventListener('wheel', (e) => {
      if (!vw) return;
      const r = box.getBoundingClientRect();
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? vh : 1;
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        if (gesture) return;
        const d = Math.max(-50, Math.min(50, e.deltaY * unit));
        zoomAt(e.clientX - r.left, e.clientY - r.top, Math.exp(-d * 0.006));
        return;
      }
      let dx = e.deltaX * unit, dy = e.deltaY * unit;
      if (e.shiftKey && dx === 0) { dx = dy; dy = 0; }
      // the wheel moves the diagram only within its snug range (or back toward it); at the end of that range, or on a
      // diagram that fits, it scrolls the page instead
      const [xl, xh] = snug(W, vw / s), [yl, yh] = snug(H, vh / s);
      const nx = within(x + dx / s, [Math.min(xl, x), Math.max(xh, x)]);
      const ny = within(y + dy / s, [Math.min(yl, y), Math.max(yh, y)]);
      if (Math.abs(nx - x) * s < 0.5 && Math.abs(ny - y) * s < 0.5) return;
      e.preventDefault();
      x = nx;
      y = ny;
      fitted = false;
      draw();
    }, { passive: false });
    // Safari's pinch on a trackpad
    box.addEventListener('gesturestart', (e) => { e.preventDefault(); gesture = true; box.dataset.g = '1'; });
    box.addEventListener('gesturechange', (e) => {
      e.preventDefault();
      const r = box.getBoundingClientRect();
      zoomAt(e.clientX - r.left, e.clientY - r.top, e.scale / Number(box.dataset.g));
      box.dataset.g = String(e.scale);
    });
    box.addEventListener('gestureend', (e) => { e.preventDefault(); gesture = false; });

    // one pointer moves the diagram, two (a pinch on a touch screen) zoom it; a press that does not move is a click
    const pts = new Map();
    let drag = null, pinch = null, moved = false;
    const capture = (id) => {
      try { if (!box.hasPointerCapture(id)) box.setPointerCapture(id); } catch { /* the pointer is already gone */ }
    };
    const two = () => {
      const [a, b] = [...pts.values()];
      return { mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2, d: Math.hypot(a.x - b.x, a.y - b.y) || 1 };
    };
    box.addEventListener('pointerdown', (e) => {
      if (e.target.closest('.pz-bar')) return;
      if (e.pointerType === 'mouse' && e.button !== 0 && e.button !== 1) return;
      if (e.button === 1) e.preventDefault();
      pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pts.size === 1) {
        drag = { id: e.pointerId, sx: e.clientX, sy: e.clientY, x, y };
        moved = false;
      } else if (pts.size === 2) {
        pinch = two();
        drag = null;
        moved = true;
        for (const id of pts.keys()) capture(id);
      }
    });
    box.addEventListener('pointermove', (e) => {
      const p = pts.get(e.pointerId);
      if (!p) return;
      p.x = e.clientX;
      p.y = e.clientY;
      if (pinch && pts.size >= 2) {
        const n = two(), r = box.getBoundingClientRect();
        x -= (n.mx - pinch.mx) / s;
        y -= (n.my - pinch.my) / s;
        zoomAt(n.mx - r.left, n.my - r.top, n.d / pinch.d);
        pinch = n;
        return;
      }
      if (!drag || drag.id !== e.pointerId) return;
      const dx = e.clientX - drag.sx, dy = e.clientY - drag.sy;
      if (!moved && Math.hypot(dx, dy) < 4) return;
      if (!moved) {
        moved = true;
        capture(e.pointerId);
        box.classList.add('pz-grabbing');
      }
      x = drag.x - dx / s;
      y = drag.y - dy / s;
      fitted = false;
      draw();
    });
    const up = (e) => {
      if (!pts.delete(e.pointerId)) return;
      if (pts.size < 2) pinch = null;
      if (pts.size === 1) {
        // the finger left on the screen after a pinch carries on moving, from where it is now
        const [[id, p]] = [...pts];
        drag = { id, sx: p.x, sy: p.y, x, y };
      } else if (pts.size === 0) {
        drag = null;
        box.classList.remove('pz-grabbing');
      }
    };
    box.addEventListener('pointerup', up);
    box.addEventListener('pointercancel', up);
    box.addEventListener('dragstart', (e) => e.preventDefault());
    box.addEventListener('click', (e) => {
      if (moved) {
        e.preventDefault();
        e.stopPropagation();
        moved = false;
        return;
      }
      // a step's link leads to its section further down the page, which the full window would hide
      if (e.target.closest('a') && host.classList.contains('pz-full')) setFull(false);
    }, true);

    box.addEventListener('keydown', (e) => {
      moved = false;
      if (e.altKey || e.ctrlKey || e.metaKey || e.target.closest('.pz-bar') && (e.key === 'Enter' || e.key === ' ')) return;
      const step = e.shiftKey ? 240 : 60;
      if (e.shiftKey && e.code === 'Digit1') fit();
      else if (e.shiftKey && e.code === 'Digit0') actual();
      else if (e.key === '+' || e.key === '=') zoomAt(vw / 2, vh / 2, 1.25);
      else if (e.key === '-' || e.key === '_') zoomAt(vw / 2, vh / 2, 0.8);
      else if (e.key === 'ArrowLeft') { x -= step / s; fitted = false; draw(); }
      else if (e.key === 'ArrowRight') { x += step / s; fitted = false; draw(); }
      else if (e.key === 'ArrowUp') { y -= step / s; fitted = false; draw(); }
      else if (e.key === 'ArrowDown') { y += step / s; fitted = false; draw(); }
      else if (e.key === 'f' || e.key === 'F') setFull(!host.classList.contains('pz-full'));
      else if (e.key === 'Escape' && host.classList.contains('pz-full')) setFull(false);
      else return;
      e.preventDefault();
    });
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && host.classList.contains('pz-full') && !box.contains(document.activeElement)) setFull(false);
    });
  }
})();</script>`;
