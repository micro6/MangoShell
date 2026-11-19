import { $, browser, expect } from "@wdio/globals";

describe("MangoShell 冒烟", () => {
  it("主窗口可加载并渲染空状态", async () => {
    await browser.waitUntil(
      async () => {
        const text = await (await $("body")).getText();
        return text.includes("开始连接");
      },
      { timeout: 20000, interval: 500, timeoutMsg: "首页在 20 秒内未渲染出空状态" },
    );

    const text = await (await $("body")).getText();
    expect(text).toContain("开始连接");
  });
});
