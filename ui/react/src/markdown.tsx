// The agent's text is Markdown. This draws it as React elements, never as HTML: what the agent
// writes between angle brackets is shown as text, so it cannot put markup or scripts on the page.

import { lexer, type Token, type Tokens } from "marked";
import { createContext, useMemo, type ReactNode } from "react";

/** A link is followed only to these. Anything else (`javascript:`, `file:`) is drawn as text. */
const SAFE_LINK = /^(https?:|mailto:)/i;

function inline(tokens: Token[] | undefined): ReactNode[] {
  return (tokens ?? []).map((token, at) => {
    const inner = () => inline((token as { tokens?: Token[] }).tokens);
    switch (token.type) {
      case "strong": return <strong key={at}>{inner()}</strong>;
      case "em": return <em key={at}>{inner()}</em>;
      case "del": return <del key={at}>{inner()}</del>;
      case "codespan": return <code key={at}>{decode((token as Tokens.Codespan).text)}</code>;
      case "br": return <br key={at} />;
      case "link": {
        const link = token as Tokens.Link;
        if (!SAFE_LINK.test(link.href)) return <span key={at}>{inner()}</span>;
        return <a key={at} href={link.href} title={link.title ?? undefined} target="_blank" rel="noreferrer noopener">{inner()}</a>;
      }
      // An image is named, not fetched: loading it would tell its server that the user is reading.
      case "image": return <span key={at} className="arc-md-image">[image: {(token as Tokens.Image).text || (token as Tokens.Image).href}]</span>;
      case "text": {
        const text = token as Tokens.Text;
        return text.tokens ? <span key={at}>{inline(text.tokens)}</span> : decode(text.text);
      }
      case "escape": return decode((token as Tokens.Escape).text);
      // Raw HTML, and anything this does not know: as written.
      default: return token.raw;
    }
  });
}

/** The parser leaves the five entities it would write into HTML; here they are characters again. */
function decode(text: string): string {
  return text.replace(/&(amp|lt|gt|quot|#39);/g, (_, name: string) => ({ amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" })[name] ?? "");
}

function blocks(tokens: Token[]): ReactNode[] {
  return tokens.map((token, at) => {
    switch (token.type) {
      case "space": return null;
      case "paragraph": return <p key={at}>{inline((token as Tokens.Paragraph).tokens)}</p>;
      case "heading": {
        const heading = token as Tokens.Heading;
        // Chat text is not a document: its headings start below the page's own.
        const Level = `h${Math.min(heading.depth + 2, 6)}` as "h3";
        return <Level key={at}>{inline(heading.tokens)}</Level>;
      }
      case "code": {
        const code = token as Tokens.Code;
        return <pre key={at} data-language={code.lang || undefined}><code>{code.text}</code></pre>;
      }
      case "blockquote": return <blockquote key={at}>{blocks((token as Tokens.Blockquote).tokens)}</blockquote>;
      case "hr": return <hr key={at} />;
      case "list": {
        const list = token as Tokens.List;
        const items = list.items.map((item, index) => (
          <li key={index}>
            {item.task && <input type="checkbox" checked={item.checked ?? false} readOnly disabled />}
            {/* A tight item's text is inline; a loose one holds paragraphs. */}
            {item.tokens.map((part, inside) => {
              // The parser also lists a task's box as a part; it is drawn above.
              if (part.type === "space" || (part.type as string) === "checkbox") return null;
              if (part.type === "text") return <span key={inside}>{inline((part as Tokens.Text).tokens ?? [part])}</span>;
              return <div key={inside}>{blocks([part])}</div>;
            })}
          </li>
        ));
        return list.ordered ? <ol key={at} start={Number(list.start) || 1}>{items}</ol> : <ul key={at}>{items}</ul>;
      }
      case "table": {
        const table = token as Tokens.Table;
        const align = (cell: Tokens.TableCell) => (cell.align ? { textAlign: cell.align } : undefined);
        return (
          <div key={at} className="arc-md-table">
            <table>
              <thead><tr>{table.header.map((cell, column) => <th key={column} style={align(cell)}>{inline(cell.tokens)}</th>)}</tr></thead>
              <tbody>
                {table.rows.map((row, line) => (
                  <tr key={line}>{row.map((cell, column) => <td key={column} style={align(cell)}>{inline(cell.tokens)}</td>)}</tr>
                ))}
              </tbody>
            </table>
          </div>
        );
      }
      case "text": return <p key={at}>{inline((token as Tokens.Text).tokens ?? [token])}</p>;
      // Raw HTML, and anything this does not know: as written.
      default: return <p key={at}>{token.raw}</p>;
    }
  });
}

/**
 * Markdown drawn as elements. Text that is still arriving is fine: a code fence that is not
 * closed yet is drawn as code up to where the text ends.
 */
export function Markdown({ text }: { text: string }) {
  const drawn = useMemo(() => blocks(lexer(text, { gfm: true })), [text]);
  return <div className="arc-md">{drawn}</div>;
}

/**
 * How the agent's text is drawn. The default is `Markdown`. Give `ChatView` a `renderText`,
 * or wrap your own tree in `<RenderText value={…}>`, to use another renderer (one with code
 * highlighting, say) or `(text) => text` for the text as written.
 */
export const RenderText = createContext<(text: string) => ReactNode>((text) => <Markdown text={text} />);
