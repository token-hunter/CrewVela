import { Children, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";
import { mentionTextSegments } from "../multiAgentMention";

/** Parse Markdown as React nodes; never execute model HTML or auto-load remote images. */
export function ConversationMarkdown({ body, mentionTokens }: { body: string; mentionTokens?: readonly string[] }) {
  const [error, setError] = useState("");
  const external = (href?: string) => !!href && /^(https?:\/\/|mailto:)/i.test(href);
  const open = (href: string) => { setError(""); void openUrl(href).catch(reason => setError(`打开链接失败：${String(reason)}`)); };
  return <div className="conversation-message-body conversation-markdown">
    <Markdown remarkPlugins={[remarkGfm]} skipHtml components={{
      p: ({ children }) => <p>{mentionTokens?.length ? Children.map(children, child => typeof child === "string"
        ? mentionTextSegments(child, mentionTokens).map((part, index) => part.mention ? <span key={index} className="conversation-message-mention">{part.text}</span> : part.text)
        : child) : children}</p>,
      a: ({ href, children }) => external(href) ? <a href={href} rel="noreferrer" onClick={event => { event.preventDefault(); open(href!); }}>{children}</a> : <span>{children}</span>,
      img: ({ src, alt }) => external(src) ? <a href={src} rel="noreferrer" onClick={event => { event.preventDefault(); open(src!); }}>查看图片{alt ? `：${alt}` : ""}</a> : <span>{alt}</span>,
      table: ({ children }) => <div className="conversation-markdown-table"><table>{children}</table></div>,
    }}>{body}</Markdown>
    {error && <p className="conversation-link-error" role="alert">{error}</p>}
  </div>;
}
