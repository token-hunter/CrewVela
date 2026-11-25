import { translate, type InterfaceLanguage } from "./interfaceLanguage";

const hasChinese = /[\u3400-\u9fff]/;
const errorPrefixes = [
  ["无法更新系统托盘的语言", "Could not update the system tray language"],
  ["无法安全退出", "Could not quit safely"],
  ["读取常规设置失败", "Could not load general settings"],
  ["读取内置记忆状态失败", "Could not read the built-in memory status"],
  ["读取浏览器控制设置失败", "Could not load browser control settings"],
  ["读取安装状态失败", "Could not read the installation status"],
  ["读取数据目录失败", "Could not read the application data directory"],
  ["打开目录失败", "Could not open the folder"],
  ["复制路径失败", "Could not copy the path"],
  ["选择项目失败", "Could not select the project"],
  ["接入检查失败", "Could not verify the integration"],
  ["更新状态复核失败，请点击检查更新", "Could not verify the update. Select Check for updates to retry"],
  ["读取进度失败，将继续重试", "Could not read progress. CrewVela will retry"],
  ["保存默认启动模式失败", "Could not save the default launch mode"],
  ["启动失败", "Could not start the session"],
  ["恢复会话失败", "Could not resume the session"],
  ["ACP 请求失败", "ACP request failed"],
  ["启动终端失败", "Could not start the terminal"],
  ["连接超时", "Connection timed out"],
  ["自动化调度失败", "Automation scheduling failed"],
  ["更新通知历史失败", "Could not update notification history"],
  ["测试通知失败", "Could not send the test notification"],
  ["保存失败", "Could not save changes"],
  ["复制失败", "Could not copy to the clipboard"],
  ["运行时不可用", "The runtime is unavailable"],
  ["记忆服务地址异常，未替换现有监听服务", "The memory service address is unavailable. The existing listener is still in place"],
  ["记忆服务认证失败", "Memory service authentication failed"],
  ["自定义证书文件不存在", "The custom certificate file does not exist"],
  ["自定义浏览器路径无效或文件不存在", "The custom browser path is invalid or the file does not exist"],
] as const;

export function localizeError(language: InterfaceLanguage, source: string, fallback = "操作失败。") {
  if (language !== "en" || !hasChinese.test(source)) return { message: source };

  const stripped = source.replace(/^Error:\s*/u, "");
  const translated = translate(language, stripped);
  if (translated !== stripped) return { message: translated };

  const memoryAuth = /记忆服务认证失败[（(]HTTP (401|403)[^）)]*[）)]/u.exec(stripped);
  if (memoryAuth) {
    return { message: `Memory service authentication failed (HTTP ${memoryAuth[1]}). Check the access token.`, details: source };
  }

  for (const [prefix, description] of errorPrefixes) {
    const separator = stripped[prefix.length];
    if (!stripped.startsWith(prefix) || (separator && !":： (.（".includes(separator))) continue;
    const suffix = stripped.slice(prefix.length).replace(/^\s*[:：]?\s*/u, "");
    if (!suffix) return { message: `${description}.` };
    const summary = translate(language, suffix);
    return hasChinese.test(summary)
      ? { message: `${description}.`, details: source }
      : { message: `${description}: ${summary}` };
  }

  const englishContext = /^([A-Za-z][^:：\r\n\u3400-\u9fff]+)[:：]/u.exec(stripped);
  return { message: englishContext ? `${englishContext[1].trim()}.` : translate(language, fallback), details: source };
}
