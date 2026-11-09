import { useEffect, useId, useRef, useState } from "react";
import type { NativeAgentKind } from "../nativeAgentSwitch";
import piVoxel from "../assets/native-agent-pi-voxel.png";
import piVoxelBody from "../assets/native-agent-pi-voxel-body.png";
import "../styles/native-agent-mascot.css";

export type NativeAgentMascotKind = Extract<NativeAgentKind, "claude_chat" | "codex_chat" | "kimi_chat" | "opencode_chat" | "pi_chat">;
type Motion = "intro" | "idle" | "story";

const stageTiming: Record<NativeAgentMascotKind, { intro: number; wait: number; story: number }> = {
  claude_chat: { intro: 4400, wait: 19000, story: 5600 },
  codex_chat: { intro: 4200, wait: 22000, story: 5600 },
  kimi_chat: { intro: 4600, wait: 17000, story: 6000 },
  opencode_chat: { intro: 4100, wait: 24000, story: 5400 },
  pi_chat: { intro: 4300, wait: 15000, story: 6200 },
};

function ClaudePaperSprite({ id }: { id: string }) {
  return <svg className="native-mascot__art native-mascot--claude" viewBox="0 0 360 280" aria-hidden="true">
    <defs>
      <linearGradient id={`${id}-clay`} x1="0" x2="1" y1="0" y2="1"><stop stopColor="#ffad72"/><stop offset=".55" stopColor="#ed7042"/><stop offset="1" stopColor="#b9472e"/></linearGradient>
      <linearGradient id={`${id}-cream`} x1="0" x2="1" y1="0" y2="1"><stop stopColor="#fff4d9"/><stop offset="1" stopColor="#dfc9a2"/></linearGradient>
      <filter id={`${id}-shadow`} x="-.3" y="-.3" width="1.6" height="1.8"><feDropShadow dx="0" dy="8" stdDeviation="6" floodColor="#000" floodOpacity=".34"/></filter>
    </defs>
    <ellipse className="native-mascot__ground-shadow" cx="181" cy="254" rx="80" ry="12" fill="#000" opacity=".28"/>
    <g className="native-mascot__figure" filter={`url(#${id}-shadow)`}>
      <g className="claude__legs"><path d="M137 210l36 4-4 29-8 12h-39l7-17z" fill={`url(#${id}-clay)`}/><path d="M184 212l37-4 10 35 13 11h-42l-10-13z" fill={`url(#${id}-clay)`}/><path d="M124 248l41 1 3 8h-49zM218 245l25 10 8 4h-48z" fill="#a94731"/></g>
      <path className="claude__body" d="M126 155l54-14 57 14 22 62-45 13-35-15-38 17-45-17z" fill={`url(#${id}-clay)`}/>
      <path d="M142 155l38-14 41 14-39 29z" fill={`url(#${id}-cream)`}/><path d="M182 184l39-29 17 6 18 48-48 17z" fill="#e8d4af"/><path d="M142 155l40 29-21 39-45-17z" fill="#f6e6c7"/>
      <path d="M108 167l-18-17-17 11 10 26 31 13 16-11z" fill="#d95c38"/><path d="M225 164l24-6 20 16-13 29-31 4-12-13z" fill="#c95033"/>
      <g className="claude__chest-mark" transform="translate(187 190)" stroke="#b74a30" strokeWidth="4" strokeLinecap="round"><path d="M0-10v20M-9-5l18 10M9-5L-9 5"/></g>
      <g className="claude__head">
        <path d="M104 37l28-13 28 9 54-7 39 17 13 32-12 65-34 22-77-5-39-23-17-48z" fill={`url(#${id}-clay)`}/>
        <path d="M104 37l28-13 28 9-17 34-39 22z" fill="#ffb17a"/><path d="M160 33l54-7 39 17-27 28-55-5z" fill="#ef7b4d"/>
        <path d="M253 43l13 32-12 65-25 19 5-77z" fill="#bd4b31"/><path d="M114 88l30-21 7 62-40-24z" fill="#dc633c"/>
        <g className="claude__eyes"><rect x="143" y="77" width="10" height="27" rx="5" fill="#321e1c"/><rect x="204" y="74" width="10" height="28" rx="5" fill="#321e1c"/></g>
        <path d="M105 86l-12 4 8 16 12-7z" fill="#f39a67"/><path d="M103 103l10 6" stroke="#a9442e" strokeWidth="2"/>
      </g>
      <g className="claude__chevron-arm"><path d="M105 161l-18-21-15 11 15 22" fill="none" stroke="#e77949" strokeWidth="18" strokeLinecap="round" strokeLinejoin="round"/><path d="M44 111l43 13-25 20 28 20" fill="none" stroke={`url(#${id}-cream)`} strokeWidth="14" strokeLinejoin="round" strokeLinecap="round"/><path d="M45 108l43 16-24 20 29 21" fill="none" stroke="#fff5de" strokeWidth="3" opacity=".75"/><path d="M84 126l-20 17 20 15" fill="none" stroke="#cfad82" strokeWidth="2" opacity=".75"/><path className="claude__paper-crease" d="M61 140l10 4" stroke="#caa77c" strokeWidth="2" opacity=".8"/></g>
      <g className="claude__fold-star"><path d="M55 103l5 11 12 1-9 8 3 12-11-6-10 6 2-12-9-8 12-1z" fill="#ffe8ae" stroke="#d7784a" strokeWidth="2"/><path d="M55 106l1 22m-10-14l18 8m-15 2l12-14" stroke="#fff8e5" strokeWidth="1.5" opacity=".8"/></g>
      <g className="claude__smoothing-hand"><path d="M104 104q-9 10-15 19l-11 6q-4 5 3 7l17-5 16-17z" fill="#f6a16f" stroke="#bd5032" strokeWidth="2"/></g>
    </g>
    <g className="native-mascot__intro-sparkles" fill="#ffe2a0"><path d="M76 53l3 9 9 3-9 3-3 9-3-9-9-3 9-3z"/><circle cx="282" cy="99" r="3"/><circle cx="92" cy="210" r="2.5"/></g>
  </svg>;
}

function CodexSprite({ id }: { id: string }) {
  return <svg className="native-mascot__art native-mascot--codex" viewBox="0 0 360 280" aria-hidden="true">
    <defs><linearGradient id={`${id}-shell`} x1="0" x2="1" y1="0" y2="1"><stop stopColor="#fff"/><stop offset=".65" stopColor="#dce9f8"/><stop offset="1" stopColor="#aebfd5"/></linearGradient><linearGradient id={`${id}-screen`} x1="0" x2="0" y1="0" y2="1"><stop stopColor="#173c70"/><stop offset="1" stopColor="#0b1c3b"/></linearGradient><filter id={`${id}-glow`}><feGaussianBlur stdDeviation="3" result="blur"/><feMerge><feMergeNode in="blur"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>
    <ellipse cx="180" cy="249" rx="75" ry="11" fill="#000" opacity=".3"/>
    <g className="native-mascot__figure codex__float">
      <g className="codex__feet"><ellipse cx="142" cy="218" rx="23" ry="15" fill={`url(#${id}-shell)`}/><path d="M124 219q18 10 36 0l-2 8q-17 9-31-1z" fill="#173d78"/><path d="M132 220h20" stroke="#31d6ff" strokeWidth="4" strokeLinecap="round"/><ellipse cx="222" cy="218" rx="23" ry="15" fill={`url(#${id}-shell)`}/><path d="M204 219q18 10 36 0l-2 8q-17 9-31-1z" fill="#173d78"/><path d="M212 220h20" stroke="#31d6ff" strokeWidth="4" strokeLinecap="round"/></g>
      <g className="codex__body"><rect x="99" y="38" width="162" height="166" rx="58" fill={`url(#${id}-shell)`} stroke="#f9fdff" strokeWidth="4"/><rect x="119" y="65" width="122" height="92" rx="32" fill={`url(#${id}-screen)`}/><path d="M143 47h73q-7 14-36 14t-37-14" fill="#bfeaff"/><rect x="111" y="99" width="7" height="32" rx="4" fill="#4adfff"/><rect x="125" y="68" width="110" height="80" rx="28" fill="#101d3a" opacity=".52"/><g className="codex__eyes" filter={`url(#${id}-glow)`} fill="#8ff5ff"><ellipse cx="158" cy="106" rx="9" ry="14"/><ellipse cx="203" cy="106" rx="9" ry="14"/></g><path className="codex__smile" d="M165 129q15 14 30 0" fill="none" stroke="#64dbff" strokeWidth="3" strokeLinecap="round"/><path d="M156 178h50" stroke="#bfdaef" strokeWidth="2" opacity=".7"/></g>
      <g className="codex__left-bracket"><path d="M82 117q-22 0-22 17v19q0 13 14 16-14 4-14 17v13q0 17 22 17" fill="none" stroke="#137eea" strokeWidth="21" strokeLinecap="round" strokeLinejoin="round"/><path d="M78 116q-20 2-20 18v18q0 13 14 17-14 4-14 18v12q0 17 21 18" fill="none" stroke="#eef8ff" strokeWidth="14" strokeLinecap="round" strokeLinejoin="round"/></g>
      <g className="codex__right-bracket"><path d="M278 117q22 0 22 17v19q0 13-14 16 14 4 14 17v13q0 17-22 17" fill="none" stroke="#137eea" strokeWidth="21" strokeLinecap="round" strokeLinejoin="round"/><path d="M282 116q20 2 20 18v18q0 13-14 17 14 4 14 18v12q0 17-21 18" fill="none" stroke="#eef8ff" strokeWidth="14" strokeLinecap="round" strokeLinejoin="round"/></g>
      <g className="codex__code-blocks"><rect className="codex__block codex__block--one" x="141" y="166" width="23" height="17" rx="4" fill="#4c9eff"/><rect className="codex__block codex__block--two" x="170" y="166" width="37" height="17" rx="4" fill="#89dfff"/><rect className="codex__block codex__block--three" x="213" y="166" width="15" height="17" rx="4" fill="#367be5"/></g>
      <g className="codex__check" fill="none" stroke="#65f1c0" strokeWidth="6" strokeLinecap="round" strokeLinejoin="round"><path d="M151 108l10 10 22-24"/><path className="codex__second-check" d="M179 108l9 9 20-22"/></g>
      <g className="codex__loose-cube"><rect x="292" y="100" width="17" height="17" rx="4" fill="#45c6ff"/><path d="M292 105l9-5 8 5m-8-5v18" fill="none" stroke="#c5f8ff" strokeWidth="2"/></g>
    </g>
    <g className="native-mascot__intro-sparkles" fill="#75e9ff"><path d="M84 58l3 8 8 3-8 3-3 8-3-8-8-3 8-3z"/><circle cx="277" cy="66" r="3"/><circle cx="93" cy="219" r="2"/></g>
  </svg>;
}

function KimiSprite({ id }: { id: string }) {
  return <svg className="native-mascot__art native-mascot--kimi" viewBox="0 0 360 280" aria-hidden="true">
    <defs><linearGradient id={`${id}-moon`} x1="0" x2="1" y1="0" y2="1"><stop stopColor="#fff9e8"/><stop offset="1" stopColor="#ddcfb5"/></linearGradient><linearGradient id={`${id}-gold`} x1="0" x2="1" y1="0" y2="1"><stop stopColor="#ffe29a"/><stop offset="1" stopColor="#d99336"/></linearGradient></defs>
    <ellipse cx="184" cy="250" rx="63" ry="10" fill="#000" opacity=".3"/>
    <g className="kimi__ears"><path d="M143 112q-44-16-43-64 1-23 18-20 30 5 54 64" fill="#26375f" stroke="#455b91" strokeWidth="5"/><path d="M198 93q18-61 52-59 20 2 17 23-7 42-56 66" fill="#1e2d50" stroke="#455b91" strokeWidth="5"/><path d="M117 43q3 31 27 48l13-17q-14-25-32-31z" fill="#111e3d"/><path d="M221 104q27-12 34-44" fill="none" stroke="#0e1a38" strokeWidth="8" strokeLinecap="round"/><g className="kimi__ear-star"><path d="M123 45l7 14 15 2-11 10 3 15-14-7-13 8 2-15-12-10 15-2z" fill="#54e4f6" stroke="#aafaff" strokeWidth="2"/></g></g>
    <g className="native-mascot__figure kimi__float">
      <g className="kimi__backpack"><ellipse cx="234" cy="174" rx="32" ry="39" fill={`url(#${id}-gold)`} stroke="#b87931" strokeWidth="4"/><circle cx="224" cy="164" r="8" fill="#f6c968" opacity=".8"/><circle cx="242" cy="185" r="6" fill="#f6c968" opacity=".8"/><path d="M211 143l-18 16m44-25l-9 10" stroke="#ffd98a" strokeWidth="4" strokeLinecap="round"/></g>
      <path className="kimi__body" d="M132 174q10-30 44-31 37 0 52 34l-4 43q-18 20-47 20-32 0-50-21z" fill="#233354"/><path d="M143 190q38 24 74 1" fill="none" stroke="#6175a3" strokeWidth="3"/><ellipse cx="154" cy="226" rx="14" ry="9" fill="#182744"/><ellipse cx="207" cy="226" rx="14" ry="9" fill="#182744"/>
      <g className="kimi__hood"><path d="M104 108q2-57 67-68 66-11 97 46 14 25-6 58-23 36-70 37-57 1-79-25-18-21-9-48z" fill={`url(#${id}-moon)`} stroke="#fff7df" strokeWidth="4"/><path d="M131 117q18-38 58-36 42 3 55 37 9 24-12 43-22 20-64 13-41-7-46-32-4-14 9-25z" fill="#172442"/><path d="M254 85q22 26 14 48" fill="none" stroke="#d6c7a9" strokeWidth="4" opacity=".8"/></g>
      <g className="kimi__face"><g className="kimi__eyes"><ellipse cx="159" cy="124" rx="8" ry="12" fill="#f1c45d"/><ellipse cx="203" cy="122" rx="8" ry="12" fill="#f1c45d"/><ellipse cx="160" cy="125" rx="4" ry="7" fill="#191b26"/><ellipse cx="204" cy="123" rx="4" ry="7" fill="#191b26"/><circle cx="162" cy="121" r="2" fill="#fff"/><circle cx="206" cy="119" r="2" fill="#fff"/></g><path d="M177 139q4-4 8 0l-4 4z" fill="#e6a184"/><path d="M178 148q5 5 10 0" fill="none" stroke="#101b33" strokeWidth="2" strokeLinecap="round"/></g>
      <g className="kimi__map-hand"><path d="M145 172q-19-2-26-16l-9-7q-7 0-4 7l9 13q11 14 30 14z" fill="#26375c" stroke="#121f3a" strokeWidth="3"/></g>
      <g className="kimi__star-map"><path d="M49 128q-3-8 8-7l56 12q5 2 4 8l-9 43q-1 6-8 4l-50-13q-5-2-4-8z" fill="#142542" stroke="#dba64c" strokeWidth="3"/><path className="kimi__constellation" d="M62 151l15-11 13 17 13-5" fill="none" stroke="#79e8f3" strokeWidth="2" strokeDasharray="50"/><g fill="#ffe7a4"><circle cx="62" cy="151" r="3"/><circle cx="77" cy="140" r="3"/><circle cx="90" cy="157" r="3"/><circle cx="103" cy="152" r="3"/></g><path d="M58 166l15 4m7-3l15 4" stroke="#8297c0" strokeWidth="2" opacity=".7"/></g>
    </g>
    <g className="native-mascot__intro-sparkles" fill="#72ebf4"><path d="M272 39l3 9 9 3-9 3-3 9-3-9-9-3 9-3z"/><circle cx="88" cy="89" r="3"/><circle cx="269" cy="207" r="2.5"/></g>
  </svg>;
}

function OpenCodeSprite({ id }: { id: string }) {
  return <svg className="native-mascot__art native-mascot--opencode" viewBox="0 0 360 280" aria-hidden="true">
    <defs><linearGradient id={`${id}-monitor`} x1="0" x2="1" y1="0" y2="1"><stop stopColor="#555963"/><stop offset=".55" stopColor="#353942"/><stop offset="1" stopColor="#252930"/></linearGradient><linearGradient id={`${id}-mint`} x1="0" x2="1"><stop stopColor="#7affe0"/><stop offset="1" stopColor="#3bd3a4"/></linearGradient></defs>
    <ellipse cx="181" cy="246" rx="75" ry="11" fill="#000" opacity=".3"/>
    <g className="opencode__legs"><path d="M131 185h29l-4 50q-2 12-15 12h-18q-8 0-8-8 0-6 8-8l11-4z" fill="#343943" stroke="#69717c" strokeWidth="3"/><path d="M209 185h30l-3 42 13 5q9 3 7 11-1 5-9 5h-23q-13-1-14-12z" fill="#30353e" stroke="#69717c" strokeWidth="3"/><path d="M122 237h29m80 0h27" stroke="#8bf3cc" strokeWidth="4" strokeLinecap="round"/></g>
    <g className="opencode__side-panel opencode__panel-left"><rect x="26" y="76" width="78" height="103" rx="12" fill="#20252a" stroke="#565e66" strokeWidth="4"/><path d="M42 100h44M42 116h31M42 132h38M42 148h24" stroke="#59ddb3" strokeWidth="4" strokeLinecap="round" opacity=".85"/><circle cx="88" cy="92" r="3" fill="#9affd9"/></g>
    <g className="opencode__side-panel opencode__panel-right"><rect x="256" y="76" width="78" height="103" rx="12" fill="#20252a" stroke="#565e66" strokeWidth="4"/><path d="M272 100h42M272 116h29M272 132h36M272 148h21" stroke="#59ddb3" strokeWidth="4" strokeLinecap="round" opacity=".85"/><circle cx="318" cy="92" r="3" fill="#9affd9"/></g>
    <g className="native-mascot__figure opencode__body"><rect x="83" y="32" width="194" height="176" rx="33" fill={`url(#${id}-monitor)`} stroke="#737b84" strokeWidth="5"/><rect x="98" y="47" width="164" height="134" rx="23" fill="#171d21" stroke="#4f575e" strokeWidth="3"/><path d="M104 51q5-6 17-3l-7 130q-8 1-11-5z" fill="#fff" opacity=".035"/><g className="opencode__eyes" fill={`url(#${id}-mint)`} filter="none"><rect x="139" y="83" width="12" height="35" rx="6"/><rect x="207" y="83" width="12" height="35" rx="6"/></g><path d="M162 132q19 16 38 0" fill="none" stroke="#53e0b3" strokeWidth="4" strokeLinecap="round"/><g className="opencode__terminal-line"><text x="117" y="169" className="opencode__prompt" fill="#75f0c8" fontSize="11" fontFamily="monospace">&gt;_</text><text x="136" y="169" className="opencode__typed" fill="#75f0c8" fontSize="10" fontFamily="monospace"><tspan className="opencode__typed-char opencode__typed-char--1">t</tspan><tspan className="opencode__typed-char opencode__typed-char--2">s</tspan><tspan className="opencode__typed-char opencode__typed-char--3">e</tspan><tspan className="opencode__typed-char opencode__typed-char--4">t</tspan></text><text x="136" y="169" className="opencode__corrected" fill="#fff1a8" fontSize="10" fontFamily="monospace"><tspan className="opencode__corrected-char opencode__corrected-char--1">t</tspan><tspan className="opencode__corrected-char opencode__corrected-char--2">e</tspan><tspan className="opencode__corrected-char opencode__corrected-char--3">s</tspan><tspan className="opencode__corrected-char opencode__corrected-char--4">t</tspan></text><path className="opencode__text-cursor" d="M136 159v12" stroke="#9affdc" strokeWidth="2"/></g><path d="M118 196h123" stroke="#87909a" strokeWidth="4" strokeLinecap="round"/><circle cx="180" cy="196" r="5" fill="#64ecc1"/><path className="opencode__cursor" d="M225 143h13v13h-13z" fill="#7affe0"/></g>
    <g className="opencode__patch-cable"><path d="M260 192q22 21 44 4l17-13" fill="none" stroke="#68717a" strokeWidth="7" strokeLinecap="round"/><path d="M313 178l12 2-2 12-12-2z" fill="#67e8bc"/><circle cx="328" cy="184" r="4" fill="#b7ffe5"/></g>
    <g className="native-mascot__intro-sparkles" fill="#69f0c3"><path d="M80 35l3 9 9 3-9 3-3 9-3-9-9-3 9-3z"/><circle cx="285" cy="52" r="3"/><circle cx="70" cy="202" r="2.5"/></g>
  </svg>;
}

function PiSprite({ id }: { id: string }) {
  return <svg className="native-mascot__art native-mascot--pi" viewBox="0 0 1434 1254" aria-hidden="true">
    <defs>
      <clipPath id={`${id}-pi-tail`}><path d="M195 413h163l42 18v58l-13 24v49l19 17v31l-22 29 2 38-23 39-4 42-39 34-41-15-34-23-31 1-31-25-27-17-25-31-17-34-43-2-15-25 4-41 19-29 2-37 22-28 19-27 26-18 25-25z"/></clipPath>
      <clipPath id={`${id}-pi-wing-back`}><path d="M1032 670h51l19 17h37l23 24 27 12 5 39 14 12v72l-15 22-9 38-31 28-29 36-38 19-40-16-14-36 14-40 3-47-18-34 8-41-13-32 6-31z"/></clipPath>
      <clipPath id={`${id}-pi-wing-front`}><path d="M300 640h50l28 20h42l25 22h41l27 27h39l23 25h32l18 27 22 26v74l-15 27v38l-20 18-10 27-29 20-26 9-36 1h-33l-29-10-33-16-29-21-31-20-27-24-21-29-22-25-12-30-4-24 12-31 23-26 12-32z"/></clipPath>
      <clipPath id={`${id}-pi-foot-left`}><path d="M509 1025h111l28 24v37l21 27 35 20v42l-18 26h-70l-24-17h-61l-29-24v-35l-12-23 16-31z"/></clipPath>
      <clipPath id={`${id}-pi-foot-right`}><path d="M764 1018h109l25 24v44l34 18 39 26v42l-24 26h-76l-23-18h-63l-26-29 1-42-11-32 15-30z"/></clipPath>
    </defs>
    <ellipse className="native-mascot__ground-shadow pi__shadow" cx="750" cy="1192" rx="265" ry="26" fill="#15100f" opacity=".24"/>
    <g className="native-mascot__figure pi__body">
      <g className="pi__tail"><image href={piVoxel} x="0" y="0" width="1254" height="1254" clipPath={`url(#${id}-pi-tail)`}/></g>
      <g className="pi__wing pi__wing--back"><image href={piVoxel} x="0" y="0" width="1254" height="1254" clipPath={`url(#${id}-pi-wing-back)`}/></g>
      <image className="pi__body-texture" href={piVoxelBody} x="0" y="0" width="1254" height="1254"/>
      <g className="pi__leg-left"><image href={piVoxel} x="0" y="0" width="1254" height="1254" clipPath={`url(#${id}-pi-foot-left)`}/></g>
      <g className="pi__leg-right"><image href={piVoxel} x="0" y="0" width="1254" height="1254" clipPath={`url(#${id}-pi-foot-right)`}/></g>
      <g className="pi__wing pi__wing--near"><image href={piVoxel} x="0" y="0" width="1254" height="1254" clipPath={`url(#${id}-pi-wing-front)`}/></g>
    </g>
    <g className="pi__built-pi" aria-hidden="true">
      <g className="pi__glyph-bar"><path d="M1206 563l17-16h140l-16 16z" fill="#f4c1ed"/><path d="M1206 563h141v25h-141z" fill="#c277ba"/><path d="M1347 563l16-16v25l-16 16z" fill="#783d73"/></g>
      <g className="pi__glyph-left"><path d="M1211 586l13-12h23l-13 12v89h-23z" fill="#e4a0da"/><path d="M1211 586h23v89h-23z" fill="#b867ad"/><path d="M1234 586l13-12v89l-13 12z" fill="#743a70"/></g>
      <g className="pi__glyph-right"><path d="M1305 586l13-12h23l-13 12v89h-23z" fill="#e4a0da"/><path d="M1305 586h23v89h-23z" fill="#b867ad"/><path d="M1328 586l13-12v89l-13 12z" fill="#743a70"/></g>
      <g className="pi__glyph-foot-left"><path d="M1203 675h23v18h-23z" fill="#743a70"/><path d="M1203 675l9-8h22l-8 8z" fill="#e4a0da"/></g>
      <g className="pi__glyph-foot-right"><path d="M1305 675h23v18h-23z" fill="#743a70"/><path d="M1305 675l9-8h22l-8 8z" fill="#e4a0da"/></g>
    </g>
    <g className="pi__fallen-pixel"><path d="M1365 725h20v20h-20z" fill="#efb4e5"/><path d="M1370 730h10v10h-10z" fill="#92518b"/></g>
    <g className="native-mascot__intro-sparkles" fill="#f5c0ed"><path d="M390 274l8 24 24 8-24 8-8 24-8-24-24-8 24-8z"/><circle cx="1104" cy="338" r="12"/><circle cx="387" cy="1000" r="9"/></g>
  </svg>;
}

export function NativeAgentMascot({ kind, active }: { kind: NativeAgentMascotKind; active: boolean }) {
  const reactId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const [motion, setMotion] = useState<Motion>("intro");
  const [reduced, setReduced] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [reacting, setReacting] = useState(false);
  const [sceneRun, setSceneRun] = useState(0);
  const lastIntroKind = useRef<NativeAgentMascotKind | null>(null);
  const reactionKind = useRef(kind);
  const phaseTimer = useRef<number | undefined>(undefined);
  const scheduleStoryRef = useRef<() => void>(() => undefined);
  const reactionTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setReduced(preference.matches);
    update();
    preference.addEventListener("change", update);
    return () => preference.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    if (!active || reduced) return;
    const timing = stageTiming[kind];
    let live = true;
    const scheduleStory = () => {
      if (!live) return;
      phaseTimer.current = window.setTimeout(() => {
        if (!live) return;
        setMotion("story");
        setSceneRun(run => run + 1);
        phaseTimer.current = window.setTimeout(() => {
          if (!live) return;
          setMotion("idle");
          scheduleStory();
        }, timing.story);
      }, timing.wait);
    };
    scheduleStoryRef.current = scheduleStory;
    if (lastIntroKind.current !== kind) {
      lastIntroKind.current = kind;
      setMotion("intro");
      phaseTimer.current = window.setTimeout(() => {
        if (!live) return;
        setMotion("idle");
        scheduleStory();
      }, timing.intro);
    } else {
      setMotion("idle");
      scheduleStory();
    }
    return () => {
      live = false;
      if (phaseTimer.current !== undefined) window.clearTimeout(phaseTimer.current);
      if (scheduleStoryRef.current === scheduleStory) scheduleStoryRef.current = () => undefined;
    };
  }, [active, kind, reduced]);

  useEffect(() => () => {
    if (reactionTimer.current !== undefined) window.clearTimeout(reactionTimer.current);
  }, []);

  useEffect(() => {
    if (active && !reduced) return;
    if (phaseTimer.current !== undefined) window.clearTimeout(phaseTimer.current);
    if (reactionTimer.current !== undefined) window.clearTimeout(reactionTimer.current);
    setReacting(false);
  }, [active, reduced]);

  useEffect(() => {
    if (reactionKind.current === kind) return;
    reactionKind.current = kind;
    if (reactionTimer.current !== undefined) window.clearTimeout(reactionTimer.current);
    setReacting(false);
  }, [kind]);

  const celebrate = () => {
    if (!active || reduced) return;
    const timing = stageTiming[kind];
    if (phaseTimer.current !== undefined) window.clearTimeout(phaseTimer.current);
    setMotion("story");
    setSceneRun(run => run + 1);
    setReacting(true);
    if (reactionTimer.current !== undefined) window.clearTimeout(reactionTimer.current);
    reactionTimer.current = window.setTimeout(() => setReacting(false), timing.story);
    phaseTimer.current = window.setTimeout(() => {
      setReacting(false);
      setMotion("idle");
      scheduleStoryRef.current();
    }, timing.story);
  };
  const artwork = kind === "claude_chat" ? <ClaudePaperSprite id={reactId} key={sceneRun}/>
    : kind === "codex_chat" ? <CodexSprite id={reactId} key={sceneRun}/>
    : kind === "kimi_chat" ? <KimiSprite id={reactId} key={sceneRun}/>
    : kind === "opencode_chat" ? <OpenCodeSprite id={reactId} key={sceneRun}/>
    : <PiSprite id={reactId} key={sceneRun}/>;
  const labels: Record<NativeAgentMascotKind, string> = {
    claude_chat: "Claude Code 纸艺小匠，点击看它折出星星",
    codex_chat: "Codex 代码精灵，点击看它整理代码方块",
    kimi_chat: "Kimi 月球探险兔，点击看它寻找星座",
    opencode_chat: "OpenCode 终端精灵，点击看它启动面板",
    pi_chat: "Pi 立体积木小鸟，点击看它搭建 π 积木",
  };
  return <button type="button" className="native-mascot" data-mascot-kind={kind.replace("_chat", "")} data-motion={motion} data-reaction={reacting ? "active" : ""} data-reduced-motion={reduced ? "true" : "false"} data-scene-run={sceneRun} aria-label={labels[kind]} title="点击触发一个小彩蛋" onClick={celebrate}>
    {artwork}
  </button>;
}
