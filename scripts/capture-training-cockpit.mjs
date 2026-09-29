import { mkdir } from "node:fs/promises";
import path from "node:path";
import { chromium } from "/Users/macforai/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright/index.mjs";

const baseUrl = process.env.COCKPIT_BASE_URL ?? "http://localhost:3001";
const outputDir = path.resolve("screenshots/训练营驾驶舱");
const chromeExecutable = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const pages = [
  { file: "01-训练营总览.png", route: "/dashboard", title: "经营总览" },
  { file: "02-期次对比.png", route: "/dashboard/cohorts", title: "期次对比" },
  { file: "03-第一期-n8n实战.png", route: "/dashboard/cohorts/1", title: "n8n 实战" },
  { file: "04-第二期-智能体实战.png", route: "/dashboard/cohorts/2", title: "智能体实战" },
  { file: "05-第三期-AI短剧.png", route: "/dashboard/cohorts/3", title: "AI 短剧" },
  { file: "06-第四期-Vibe-Coding.png", route: "/dashboard/cohorts/4", title: "Vibe Coding" },
  { file: "07-第五期-OpenClaw.png", route: "/dashboard/cohorts/5", title: "OpenClaw" },
  { file: "08-第六期-IP影视创作.png", route: "/dashboard/cohorts/6", title: "IP 影视创作" },
  { file: "09-第七期-AI硬件.png", route: "/dashboard/cohorts/7", title: "AI 硬件" },
  { file: "10-作业分析.png", route: "/dashboard/assignments", title: "作业分析" },
  { file: "11-校友会.png", route: "/dashboard/alumni", title: "校友会" },
  { file: "12-优秀学员列表.png", route: "/dashboard/outstanding-students", title: "优秀学员列表" },
  { file: "13-学员画像.png", route: "/dashboard/learners", title: "学员画像" },
  { file: "14-数据质量与预警.png", route: "/dashboard/data-quality", title: "数据质量与预警" },
];

await mkdir(outputDir, { recursive: true });

const browser = await chromium.launch({
  executablePath: chromeExecutable,
  headless: true,
});

const context = await browser.newContext({
  viewport: { width: 1440, height: 1000 },
  deviceScaleFactor: 1,
  locale: "zh-CN",
  colorScheme: "light",
});

try {
  for (const item of pages) {
    const page = await context.newPage();
    const response = await page.goto(`${baseUrl}${item.route}`, {
      waitUntil: "domcontentloaded",
      timeout: 30_000,
    });
    if (!response?.ok()) {
      throw new Error(`${item.route} returned HTTP ${response?.status() ?? "unknown"}`);
    }

    await page.locator("main.shell").waitFor({ state: "visible", timeout: 20_000 });
    await page.getByRole("heading", { name: item.title, exact: true }).first().waitFor({
      state: "visible",
      timeout: 20_000,
    });
    await page.evaluate(async () => {
      await document.fonts.ready;
      window.scrollTo(0, 0);
    });
    await page.addStyleTag({
      content: "*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; }",
    });

    const target = path.join(outputDir, item.file);
    await page.screenshot({ path: target, fullPage: true, animations: "disabled" });
    const dimensions = await page.evaluate(() => ({
      width: document.documentElement.scrollWidth,
      height: document.documentElement.scrollHeight,
    }));
    console.log(`${item.file}\t${item.route}\t${dimensions.width}x${dimensions.height}`);
    await page.close();
  }
} finally {
  await context.close();
  await browser.close();
}

