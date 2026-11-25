// Terminal output as styled pieces: the colour codes a command prints are read, the rest of the
// control codes are dropped. Not a terminal emulator: nothing moves the cursor except a return
// to the start of the line, which is how progress bars redraw.

export interface AnsiStyle {
  /** 0 to 15 for the terminal's own colours, or a CSS colour for the others. */
  fg?: number | string;
  bg?: number | string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  inverse?: boolean;
}

export interface AnsiPiece {
  text: string;
  style: AnsiStyle;
}

/** A colour of the 256-colour table as CSS, for the ones beyond the terminal's own 16. */
function indexed(n: number): number | string {
  if (n < 16) return n;
  if (n >= 232) {
    const grey = 8 + (n - 232) * 10;
    return `rgb(${grey},${grey},${grey})`;
  }
  const step = (value: number) => (value === 0 ? 0 : 55 + value * 40);
  const at = n - 16;
  return `rgb(${step(Math.floor(at / 36))},${step(Math.floor(at / 6) % 6)},${step(at % 6)})`;
}

function applySgr(style: AnsiStyle, codes: number[]): AnsiStyle {
  let next = { ...style };
  for (let at = 0; at < codes.length; at += 1) {
    const code = codes[at]!;
    if (code === 0) next = {};
    else if (code === 1) next.bold = true;
    else if (code === 2) next.dim = true;
    else if (code === 3) next.italic = true;
    else if (code === 4) next.underline = true;
    else if (code === 7) next.inverse = true;
    else if (code === 22) { delete next.bold; delete next.dim; }
    else if (code === 23) delete next.italic;
    else if (code === 24) delete next.underline;
    else if (code === 27) delete next.inverse;
    else if (code >= 30 && code <= 37) next.fg = code - 30;
    else if (code >= 90 && code <= 97) next.fg = code - 90 + 8;
    else if (code === 39) delete next.fg;
    else if (code >= 40 && code <= 47) next.bg = code - 40;
    else if (code >= 100 && code <= 107) next.bg = code - 100 + 8;
    else if (code === 49) delete next.bg;
    else if (code === 38 || code === 48) {
      const key = code === 38 ? "fg" : "bg";
      if (codes[at + 1] === 5 && codes[at + 2] !== undefined) {
        next[key] = indexed(codes[at + 2]!);
        at += 2;
      } else if (codes[at + 1] === 2 && codes[at + 4] !== undefined) {
        next[key] = `rgb(${codes[at + 2]},${codes[at + 3]},${codes[at + 4]})`;
        at += 4;
      }
    }
  }
  return next;
}

// CSI (ESC [ … letter), OSC (ESC ] … BEL or ESC \), and two-character escapes.
const ESCAPE = /\u001b\[([0-9;:?]*)([@-~])|\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)|\u001b[@-Z\\^_]/g;

/** Applies returns to the start of a line: what is printed after one replaces what was there. */
function withReturns(text: string): string {
  if (!text.includes("\r")) return text;
  return text.split("\n").map((line) => {
    const parts = line.replace(/\r+$/, "").split("\r");
    return parts.reduce((shown, part) => part + shown.slice(part.length), "");
  }).join("\n");
}

/** Splits terminal output into pieces of one style each. */
export function ansiPieces(output: string): AnsiPiece[] {
  const pieces: AnsiPiece[] = [];
  let style: AnsiStyle = {};
  let from = 0;
  const push = (text: string) => {
    const shown = withReturns(text).replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, "");
    if (shown) pieces.push({ text: shown, style });
  };
  for (const found of output.matchAll(ESCAPE)) {
    push(output.slice(from, found.index));
    from = found.index + found[0].length;
    if (found[2] === "m") style = applySgr(style, (found[1] ?? "").split(/[;:]/).map((code) => Number(code) || 0));
  }
  push(output.slice(from));
  return pieces;
}
