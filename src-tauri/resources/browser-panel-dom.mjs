// Runs only inside the dedicated WKWebView. Never receives local bridge credentials.
// Synchronous on purpose: the MCP adapter owns bounded navigation/text waiting.
export function panelCommand(action, args = {}) {
  const key = '__crewvelaPanelDomV1';
  const state = window[key] ??= { documentId: `${Date.now()}-${Math.random()}`, generation: 0, refs: new Map() };
  const text = value => String(value ?? '').replace(/\s+/g, ' ').trim();
  const visible = el => !el.hidden && !el.closest('[inert],[aria-hidden="true"]') &&
    getComputedStyle(el).visibility !== 'hidden' && el.getClientRects().length > 0;
  const labelText = label => {
    const copy = label.cloneNode(true);
    for (const control of copy.querySelectorAll('input,textarea,select,button')) control.remove();
    return copy.textContent;
  };
  const name = el => text(el.getAttribute('aria-label') ||
    (el.getAttribute('aria-labelledby') || '').split(/\s+/).map(id => el.ownerDocument.getElementById(id)?.textContent || '').join(' ') ||
    [...(el.labels || [])].map(labelText).join(' ') || el.alt || el.getAttribute('title') ||
    (el.matches('input,textarea') ? el.placeholder : el.innerText || el.textContent));
  const role = el => el.getAttribute('role') || ({ A: 'link', BUTTON: 'button', SELECT: 'combobox', TEXTAREA: 'textbox',
    SUMMARY: 'button', H1: 'heading', H2: 'heading', H3: 'heading', H4: 'heading', H5: 'heading', H6: 'heading' }[el.tagName]) ||
    (el.matches('input') ? ({ checkbox: 'checkbox', radio: 'radio', button: 'button', submit: 'button', range: 'slider' }[el.type] || 'textbox') : 'element');
  const target = ref => {
    const el = state.refs.get(ref);
    if (!el || !el.isConnected || !visible(el)) throw new Error('元素 ref 已失效或不可见，请重新 browser_snapshot');
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') throw new Error('元素已禁用');
    return el;
  };
  const dispatch = (el, kind) => el.dispatchEvent(new Event(kind, { bubbles: true, composed: true }));
  const fill = (el, value) => {
    el.focus();
    if (el.isContentEditable) { el.textContent = value; dispatch(el, 'input'); return; }
    if (!el.matches('input,textarea')) throw new Error('该 ref 不是文本输入框');
    if (['file','hidden','submit','button','checkbox','radio'].includes(el.type)) throw new Error('该输入类型不能作为文本填写');
    if (el.readOnly) throw new Error('输入框为只读');
    const win = el.ownerDocument.defaultView;
    const proto = el.tagName === 'TEXTAREA' ? win.HTMLTextAreaElement.prototype : win.HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value);
    dispatch(el, 'input'); dispatch(el, 'change');
  };
  const press = (el, value) => {
    if (!['Enter', 'Tab', 'Shift+Tab', 'Escape'].includes(value)) throw new Error('面板按键仅支持 Enter、Tab、Shift+Tab、Escape；文本请用 browser_type');
    const parts = value.split('+'); const key = parts.pop();
    const options = { key, bubbles: true, cancelable: true, ctrlKey: parts.includes('Control'),
      metaKey: parts.includes('Meta'), shiftKey: parts.includes('Shift'), altKey: parts.includes('Alt') };
    const accepted = el.dispatchEvent(new KeyboardEvent('keydown', options));
    if (accepted && key === 'Enter' && el.matches('input') && el.form) el.form.requestSubmit();
    if (accepted && key === 'Enter' && el.matches('a,button,summary')) el.click();
    if (accepted && key === 'Tab') {
      const candidates = [...el.ownerDocument.querySelectorAll('a[href],button,input,textarea,select,[tabindex]')]
        .filter(item => visible(item) && !item.disabled && item.tabIndex >= 0);
      const index = candidates.indexOf(el); candidates[(index + (options.shiftKey ? -1 : 1) + candidates.length) % candidates.length]?.focus();
    }
    el.dispatchEvent(new KeyboardEvent('keyup', options));
  };
  if (action === 'info') return { url: location.href, title: document.title, ready: document.readyState, documentId: state.documentId };
  if (action === 'snapshot') {
    state.refs.clear(); state.generation++;
    const lines = [`URL: ${location.href}`, `Title: ${document.title}`, 'DOM snapshot (visible text and controls):', (document.body?.innerText || '').slice(0, 32000)];
    const visit = root => {
      for (const el of root.querySelectorAll('*')) {
        if (state.refs.size >= 2000) return;
        if (!visible(el)) continue;
        if (el.matches('a[href],button,input:not([type=hidden]),textarea,select,summary,[role],[contenteditable=true],[onclick],h1,h2,h3,h4,h5,h6,[tabindex]')) {
          const ref = `${state.documentId}-s${state.generation}e${state.refs.size + 1}`;
          state.refs.set(ref, el);
          lines.push(`- ${role(el)} ${JSON.stringify(name(el).slice(0, 500))} [ref=${ref}]${el.checked ? ' [checked]' : ''}${el.disabled ? ' [disabled]' : ''}${el.type === 'password' ? '' : el.matches('input,textarea,select') ? ` value=${JSON.stringify(el.value)}` : ''}`);
          if (state.refs.size >= 2000) { lines.push('[control limit reached]'); break; }
        }
        if (el.shadowRoot) visit(el.shadowRoot);
        if (el.tagName === 'IFRAME') {
          try { if (el.contentDocument) visit(el.contentDocument); else lines.push('[cross-origin iframe: not accessible]'); }
          catch { lines.push('[cross-origin iframe: not accessible]'); }
        }
      }
    };
    if (document.body) visit(document.body);
    return lines.join('\n');
  }
  if (action === 'text') return document.body?.innerText || '';
  if (action === 'evaluate') {
    const fn = (0, eval)(`(${args.function})`);
    if (typeof fn !== 'function') throw new Error('function 必须为 JavaScript 函数');
    const result = args.ref ? fn(target(args.ref)) : fn();
    if (result && typeof result.then === 'function') throw new Error('WKWebView 面板暂不支持异步 evaluate，请使用 browser_wait_for');
    return result ?? null;
  }
  if (action === 'scroll') { window.scrollBy(args.x || 0, args.y || 0); return true; }
  if (action === 'press') { press(args.ref ? target(args.ref) : document.activeElement || document.body, args.key); return true; }
  if (action === 'fill_form') {
    // Validate every target before the first mutation.
    const fields = args.fields.map(field => ({ ...field, el: target(field.ref) }));
    for (const field of fields) {
      if (['checkbox', 'radio'].includes(field.type)) {
        if (field.el.tagName !== 'INPUT' || field.el.type !== field.type || !['true','false'].includes(field.value)) throw new Error('勾选字段类型或布尔值无效');
        if (field.type === 'radio' && field.value === 'false') throw new Error('radio 只能通过选择其他选项取消');
      } else if (field.type === 'combobox') {
        if (field.el.tagName !== 'SELECT' || ![...field.el.options].some(option => option.value === field.value)) throw new Error('下拉字段类型或选项无效');
      } else if (field.type !== 'textbox' || !(field.el.matches('input,textarea') || field.el.isContentEditable) || field.el.readOnly || ['file','hidden','submit','button','checkbox','radio'].includes(field.el.type)) {
        throw new Error('文本字段类型无效或只读');
      }
    }
    for (const field of fields) {
      if (field.type === 'checkbox' || field.type === 'radio') {
        const checked = field.value === 'true'; if (field.el.checked !== checked) field.el.click();
      } else if (field.type === 'combobox') {
        field.el.value = field.value; dispatch(field.el, 'input'); dispatch(field.el, 'change');
      } else fill(field.el, field.value);
    }
    return true;
  }
  const el = target(args.ref);
  el.scrollIntoView({ block: 'center', inline: 'nearest' });
  switch (action) {
    case 'click':
      if (args.button && args.button !== 'left') throw new Error('WKWebView DOM 点击仅支持左键');
      // Keep navigation in the visible single-page panel.
      if (el.closest('a')) el.closest('a').target = '_self';
      el.focus(); el.click();
      if (args.doubleClick) { el.click(); el.dispatchEvent(new MouseEvent('dblclick', { bubbles: true })); }
      break;
    case 'type': fill(el, args.text); if (args.submit) press(el, 'Enter'); break;
    case 'select': {
      if (!el.matches('select')) throw new Error('该 ref 不是 select');
      const values = new Set(args.values);
      if ([...values].some(value => ![...el.options].some(option => option.value === value))) throw new Error('下拉选项不存在');
      for (const option of el.options) option.selected = values.has(option.value);
      dispatch(el, 'input'); dispatch(el, 'change'); break;
    }
    case 'hover': el.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })); el.dispatchEvent(new MouseEvent('mouseenter')); break;
    default: throw new Error('未知 DOM 操作');
  }
  return true;
}

export function panelScript(action, args = {}) {
  return `(() => { try { return { ok: true, value: (${panelCommand.toString()})(${JSON.stringify(action)}, ${JSON.stringify(args)}) }; } catch (error) { return { ok: false, error: String(error.message || error) }; } })()`;
}
