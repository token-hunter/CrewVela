export async function openConversationTools(page) {
  const trigger = page.getByRole("button", { name: "聊天工具", exact: true });
  if (await trigger.getAttribute("aria-expanded") !== "true") await trigger.click();
  return page.getByRole("menu", { name: "聊天工具", exact: true });
}

export async function chooseConversationTool(page, name) {
  const menu = await openConversationTools(page);
  await menu.getByRole("menuitem", { name, exact: true }).click();
}
