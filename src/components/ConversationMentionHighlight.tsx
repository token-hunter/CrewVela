import { useLayoutEffect, useRef, type RefObject } from "react";
import type { MentionTextSegment } from "../multiAgentMention";
import "../styles/conversation-mention-highlight.css";

/** A non-interactive mirror leaves native textarea editing, IME and clipboard intact. */
export function ConversationMentionHighlight({textareaRef,segments}: {textareaRef:RefObject<HTMLTextAreaElement>;segments:MentionTextSegment[]}) {
  const viewport = useRef<HTMLDivElement>(null);
  const text = useRef<HTMLDivElement>(null);
  useLayoutEffect(()=>{
    const input=textareaRef.current, frame=viewport.current, content=text.current;
    if (!input || !frame || !content) return;
    const scroll=()=>{content.style.transform=`translate(${-input.scrollLeft}px, ${-input.scrollTop}px)`;};
    const measure=()=>{
      const style=getComputedStyle(input);
      Object.assign(frame.style,{left:`${input.offsetLeft+input.clientLeft}px`,top:`${input.offsetTop+input.clientTop}px`,width:`${input.clientWidth}px`,height:`${input.clientHeight}px`});
      Object.assign(content.style,{width:`${input.clientWidth}px`,minHeight:`${input.clientHeight}px`,font:style.font,lineHeight:style.lineHeight,letterSpacing:style.letterSpacing,wordSpacing:style.wordSpacing,textAlign:style.textAlign,textIndent:style.textIndent,textTransform:style.textTransform,direction:style.direction,tabSize:style.tabSize,padding:style.padding,whiteSpace:style.whiteSpace,overflowWrap:style.overflowWrap,wordBreak:style.wordBreak});
      scroll();
    };
    measure();
    const observer=new ResizeObserver(measure);observer.observe(input);
    input.addEventListener("scroll",scroll);window.addEventListener("resize",measure);
    return()=>{observer.disconnect();input.removeEventListener("scroll",scroll);window.removeEventListener("resize",measure);};
  },[textareaRef,segments]);
  return <div ref={viewport} className="conversation-mention-highlight" aria-hidden="true"><div ref={text}>{segments.map((part,index)=>part.mention?<mark key={index}>{part.text}</mark>:part.text)}<span>{"\u200b"}</span></div></div>;
}
