// The page of a flow: its styles, shared with the index and the navigation pages, and the page itself
// (crates/delurio-page). renderPage and callLine answer with an error until the port of html.ts is in this build.
import { call, native } from './internal.js';

export const CSS = native.pageCss();

export function callLine(_a) {
  return call(null, () => native.pageNotPorted('callLine'));
}

export function renderPage(_p) {
  return call(null, () => native.pageNotPorted('renderPage'));
}
