// SPDX-License-Identifier: GPL-3.0-or-later
// End-to-end test of the browser build in headless Chromium (Playwright).
//
//   scripts/build-web.sh
//   target/debug/xeentool synth /tmp/synth        # sample archive to upload
//   node scripts/test-web.cjs dist/web /tmp/synth/xeen.cc [screenshot-dir]
const http = require("http");
const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

const [root, sampleCc, shotDir] = process.argv.slice(2);
if (!root || !sampleCc) {
  console.error("usage: node scripts/test-web.cjs <dist/web> <xeen.cc> [screenshot-dir]");
  process.exit(2);
}
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm" };

function serve() {
  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => {
      const rel = decodeURIComponent(req.url.split("?")[0]).replace(/^\/+/, "") || "index.html";
      const file = path.join(root, rel);
      fs.readFile(file, (err, data) => {
        if (err) {
          res.writeHead(404);
          res.end();
          return;
        }
        res.writeHead(200, { "Content-Type": TYPES[path.extname(file)] || "application/octet-stream" });
        res.end(data);
      });
    });
    srv.listen(0, "127.0.0.1", () => resolve(srv));
  });
}

// Web fonts are optional (every face has a fallback) and CI sandboxes often
// block them; any other failed request or script error fails the test.
const OPTIONAL = /fonts\.(googleapis|gstatic)\.com/;
function watch(page, errors) {
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("requestfailed", (r) => OPTIONAL.test(r.url()) || errors.push("request failed: " + r.url()));
  page.on("console", (m) => {
    if (m.type() === "error" && !/Failed to load resource/.test(m.text())) errors.push(m.text());
  });
}

function check(cond, msg) {
  if (!cond) {
    throw new Error("FAILED: " + msg);
  }
  console.log("ok   " + msg);
}

async function canvasStats(page) {
  return page.evaluate(() => {
    const c = document.getElementById("screen");
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    const colours = new Set();
    let sum = 0;
    for (let i = 0; i < d.length; i += 4) {
      colours.add((d[i] << 16) | (d[i + 1] << 8) | d[i + 2]);
      sum += d[i] + d[i + 1] + d[i + 2];
    }
    return { colours: colours.size, sum };
  });
}

(async () => {
  const srv = await serve();
  const url = `http://127.0.0.1:${srv.address().port}/index.html`;
  const browser = await chromium.launch();
  const errors = [];
  try {
    for (const scheme of ["light", "dark"]) {
      const page = await browser.newPage({ viewport: { width: 1280, height: 900 }, colorScheme: scheme });
      watch(page, errors);
      await page.goto(url);
      await page.waitForFunction(() => /demo/i.test(document.getElementById("status").textContent), null, {
        timeout: 15000,
      });
      await page.waitForTimeout(700);
      const demo = await canvasStats(page);
      check(demo.colours > 8, `${scheme}: demo draws (${demo.colours} colours)`);
      if (shotDir) await page.screenshot({ path: path.join(shotDir, `web-demo-${scheme}.png`), fullPage: true });

      if (scheme === "light") {
        // Sample data -> asset browser, keyboard navigation changes the picture.
        await page.click("#sample");
        await page.waitForFunction(() => /asset browser/i.test(document.getElementById("status").textContent));
        await page.waitForTimeout(200);
        const list = await canvasStats(page);
        await page.focus("#screen");
        await page.keyboard.press("Enter");
        await page.waitForTimeout(200);
        const opened = await canvasStats(page);
        check(opened.sum !== list.sum, "Enter opens the selected entry");
        await page.keyboard.press("ArrowUp");
        await page.keyboard.press("ArrowRight");
        await page.waitForTimeout(200);
        if (shotDir) await page.screenshot({ path: path.join(shotDir, "web-sprite.png") });

        // Upload a real archive through the file picker.
        await page.setInputFiles("#files", sampleCc);
        await page.waitForFunction(() => document.querySelectorAll("#file-list li").length === 1);
        await page.waitForFunction(() => /asset browser/i.test(document.getElementById("status").textContent));
        check(true, "file picker loads an archive");
        await page.focus("#screen");
        for (let i = 0; i < 8; i++) await page.keyboard.press("ArrowDown");
        await page.keyboard.press("Enter");
        await page.waitForTimeout(250);
        if (shotDir) await page.screenshot({ path: path.join(shotDir, "web-files.png") });

        // Escape twice: leave the viewer, then quit.
        await page.keyboard.press("Escape");
        await page.keyboard.press("Escape");
        await page.waitForFunction(() => /stopped/i.test(document.getElementById("status").textContent));
        check(true, "Esc on the list stops the engine");
      }
      await page.close();
    }

    // Phone width: no horizontal scrolling.
    const phone = await browser.newPage({ viewport: { width: 400, height: 860 }, deviceScaleFactor: 2 });
    watch(phone, errors);
    await phone.goto(url);
    await phone.waitForFunction(() => /demo/i.test(document.getElementById("status").textContent));
    const overflow = await phone.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    check(overflow <= 0, `phone layout has no horizontal scroll (overflow ${overflow}px)`);
    if (shotDir) await phone.screenshot({ path: path.join(shotDir, "web-phone.png"), fullPage: true });

    check(errors.length === 0, "no page errors" + (errors.length ? ": " + errors.join(" | ") : ""));
  } finally {
    await browser.close();
    srv.close();
  }
})().catch((e) => {
  console.error(e.message || e);
  process.exit(1);
});
