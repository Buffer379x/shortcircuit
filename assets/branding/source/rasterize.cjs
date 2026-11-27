// Rasterizes the brand SVGs listed in ../raster.json with Chromium. Run through Playwright.
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PW || '/deps/node_modules/playwright');
const root = path.resolve(__dirname, '..');
(async () => {
  const jobs = JSON.parse(fs.readFileSync(path.join(root, 'raster.json'), 'utf8'));
  fs.mkdirSync(path.join(root, 'png'), { recursive: true });
  const browser = await chromium.launch();
  const page = await browser.newPage();
  for (const [svg, out, w, h0, transparent] of jobs) {
    const src = fs.readFileSync(path.join(root, 'svg', svg), 'utf8');
    const [, , vw, vh] = src.match(/viewBox="([^"]+)"/)[1].split(' ').map(Number);
    const h = h0 || Math.round((w * vh) / vw);
    await page.setViewportSize({ width: w, height: h });
    await page.setContent(`<style>html,body{margin:0;background:transparent}svg{display:block;width:${w}px;height:${h}px}</style>${src}`);
    await page.screenshot({ path: path.join(root, 'png', out), omitBackground: transparent });
  }
  await browser.close();
  console.log(jobs.length, 'png written');
})();
