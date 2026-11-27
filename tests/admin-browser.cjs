// Browser regression test for the admin interface and the public pages, run against a built
// container with synthetic data. It also runs axe-core (WCAG 2.2 A and AA) over the main routes in
// the dark and light theme and fails on any violation, page error, or CSP violation.
//
// Environment: SHORTCIRCUIT_PLAYWRIGHT (path to the playwright package), SHORTCIRCUIT_SETUP_TOKEN
// (from `shortcircuit setup-token`), and SHORTCIRCUIT_AXE_CORE (path to the axe-core package).
const assert = require('node:assert/strict');
const { readFile } = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require(process.env.SHORTCIRCUIT_PLAYWRIGHT);

const admin = 'http://localhost:8081';
const publicOrigin = 'http://localhost:8080';
const setupToken = process.env.SHORTCIRCUIT_SETUP_TOKEN;
const axeDirectory = process.env.SHORTCIRCUIT_AXE_CORE;
const adminPassword = 'ci-browser-password-example';
const axeTags = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22a', 'wcag22aa'];

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const startsWith = (text) => new RegExp('^' + escapeRegExp(text));

// Collects uncaught page errors and Content Security Policy violations of one page.
function watch(page, errors) {
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if (message.type() === 'error' && /Content Security Policy/i.test(message.text())) {
      errors.push(message.text());
    }
  });
}

// The admin API allows 240 requests per minute per client by default (RATE_LIMIT_API). The test
// works much faster than a person, so a token bucket paces the browser's API requests instead of
// raising the limit: a burst of 30, then 3 per second, keeps any minute at 210 or fewer and leaves
// room for the direct `page.request` calls. A paced request waits a fraction of a second.
const apiPace = { tokens: 30, capacity: 30, perSecond: 3, at: Date.now() };
async function paceApi(context) {
  await context.route(url => url.origin === admin && url.pathname.startsWith('/api/'), async route => {
    for (;;) {
      const now = Date.now();
      apiPace.tokens = Math.min(apiPace.capacity, apiPace.tokens + (now - apiPace.at) / 1000 * apiPace.perSecond);
      apiPace.at = now;
      if (apiPace.tokens >= 1) break;
      await new Promise(resolve => setTimeout(resolve, (1 - apiPace.tokens) / apiPace.perSecond * 1000 + 5));
    }
    apiPace.tokens -= 1;
    await route.continue();
  });
}

async function noHorizontalScroll(page) {
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
}

async function verify() {
  assert.match(setupToken, /^[a-f0-9]{64}$/);
  assert.ok(axeDirectory, 'SHORTCIRCUIT_AXE_CORE must point to the axe-core package.');
  const axeSource = await readFile(path.join(axeDirectory, 'axe.min.js'), 'utf8');
  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({ viewport: { width: 1360, height: 900 } });
    await paceApi(context);
    const page = await context.newPage();
    await page.addInitScript(() => {
      const request = window.fetch.bind(window);
      window.fetch = (url, options) => {
        if (!window.delayNextLogoPreview || !String(url).includes('/qr/logo?format=png')) {
          return request(url, options);
        }
        window.delayNextLogoPreview = false;
        window.delayedLogoPreview = true;
        return new Promise((_resolve, reject) => {
          if (!options?.signal) {
            reject(new Error('A logo preview must be cancellable.'));
            return;
          }
          options.signal.addEventListener('abort', () => {
            window.abortedLogoPreview = true;
            reject(options.signal.reason);
          }, { once: true });
        });
      };
    });
    const errors = [];
    watch(page, errors);

    // Below 900 px the sidebar is a drawer behind the Open navigation button.
    const navigate = async (name) => {
      const menu = page.getByRole('button', { name: 'Open navigation', exact: true });
      const drawer = await menu.isVisible();
      if (drawer) {
        await menu.click();
        assert.equal(await menu.getAttribute('aria-expanded'), 'true');
      }
      await page.locator('#main-navigation').getByRole('link', { name: new RegExp('^' + escapeRegExp(name) + '(\\s|$)') }).click();
      if (drawer) {
        await page.waitForFunction(() => document.querySelector('.menu-button')?.getAttribute('aria-expanded') === 'false');
      }
    };
    const heading = (name) => page.getByRole('heading', { level: 1, name, exact: true });
    const toast = (title) => page.locator('.toasts').getByText(title, { exact: true });
    const newLinkButton = page.locator('header.topbar').getByRole('button', { name: 'New link', exact: true });
    const linkTable = page.locator('table.links-table');
    const row = (shortUrl) => linkTable.locator('tbody tr').filter({ hasText: shortUrl });
    const rowAction = async (slug, item) => {
      await row('localhost/' + slug).getByRole('button', { name: `More actions for ${slug}`, exact: true }).click();
      await page.getByRole('menu', { name: `More actions for ${slug}`, exact: true })
        .getByRole('menuitem', { name: item, exact: true }).click();
    };
    const chooseFile = async (button, file) => {
      const [chooser] = await Promise.all([page.waitForEvent('filechooser'), button.click()]);
      await chooser.setFiles(file);
    };
    const createdLink = () => page.waitForResponse(response =>
      response.url() === admin + '/api/v1/links' && response.request().method() === 'POST');

    // ---------- Shell, headers, and static assets ----------
    const pageResponse = await page.goto(admin);
    assert.ok(pageResponse.ok());
    assert.match(pageResponse.headers()['content-security-policy'], /frame-ancestors 'none'/);
    assert.match(pageResponse.headers()['content-security-policy'], /style-src 'self'/);
    // The only frame source is the admin origin, for the sandboxed public page preview.
    assert.match(pageResponse.headers()['content-security-policy'], /frame-src 'self';/);
    for (const [name, type] of [['favicon.svg', 'image/svg+xml'], ['favicon.png', 'image/png']]) {
      assert.equal(await page.locator(`link[rel="icon"][href="/${name}"]`).count(), 1);
      const favicon = await page.request.get(admin + '/' + name);
      assert.equal(favicon.status(), 200);
      assert.equal(favicon.headers()['content-type'], type);
      assert.deepEqual(await favicon.body(), await readFile('assets/branding/' + name));
      assert.equal((await page.request.get(publicOrigin + '/' + name)).status(), 404);
    }
    assert.equal(await page.locator('link[rel="manifest"][href="/manifest.webmanifest"]').count(), 1);
    const manifestResponse = await page.request.get(admin + '/manifest.webmanifest');
    assert.equal(manifestResponse.status(), 200);
    assert.equal(manifestResponse.headers()['content-type'], 'application/manifest+json');
    const manifest = await manifestResponse.json();
    assert.ok(manifest.icons.length >= 2);
    for (const icon of [...manifest.icons, { src: '/apple-touch-icon.png' }]) {
      const image = await page.request.get(new URL(icon.src, admin).href);
      assert.equal(image.status(), 200, icon.src);
      assert.equal(image.headers()['content-type'], 'image/png', icon.src);
      assert.equal((await image.body()).subarray(0, 8).toString('hex'), '89504e470d0a1a0a', icon.src);
    }
    assert.equal((await page.request.get(publicOrigin + '/manifest.webmanifest')).status(), 404);
    for (const font of ['geist-latin-wght-normal', 'geist-latin-ext-wght-normal', 'geist-mono-latin-wght-normal']) {
      const response = await page.request.get(`${admin}/assets/${font}.woff2`);
      assert.equal(response.status(), 200, font);
      assert.equal(response.headers()['content-type'], 'font/woff2', font);
      assert.equal((await response.body()).subarray(0, 4).toString(), 'wOF2', font);
      assert.equal((await page.request.get(`${publicOrigin}/assets/${font}.woff2`)).status(), 404);
    }
    // The script and stylesheet are served precompressed, chosen by Accept-Encoding.
    for (const asset of ['/assets/admin.js', '/assets/admin.css']) {
      const plain = await page.request.get(admin + asset, { headers: { 'Accept-Encoding': 'identity' } });
      assert.equal(plain.status(), 200);
      assert.equal(plain.headers()['content-encoding'], undefined);
      assert.match(plain.headers().vary, /accept-encoding/i);
      const original = await plain.body();
      for (const [accept, encoding] of [['br', 'br'], ['gzip', 'gzip'], ['gzip, deflate, br', 'br'], ['br;q=0, gzip', 'gzip']]) {
        const encoded = await page.request.get(admin + asset, { headers: { 'Accept-Encoding': accept } });
        assert.equal(encoded.status(), 200);
        assert.equal(encoded.headers()['content-encoding'], encoding, `${asset} ${accept}`);
        assert.match(encoded.headers().vary, /accept-encoding/i);
        assert.deepEqual(await encoded.body(), original, `${asset} ${accept}`);
      }
    }

    // ---------- Setup ----------
    for (const theme of ['light', 'dark']) {
      await page.evaluate(value => localStorage.setItem('shortcircuit-theme', value), theme);
      await page.reload();
      await page.getByLabel('Setup token', { exact: true }).waitFor();
      assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), theme);
      await auditPage(page, 'Setup', axeSource);
    }
    await page.getByLabel('Setup token', { exact: true }).fill(setupToken);
    await page.getByRole('button', { name: 'Continue', exact: true }).click();
    await page.getByLabel('Username', { exact: true }).fill('Test administrator');
    await page.getByLabel('Password', { exact: true }).fill(adminPassword);
    await page.getByRole('button', { name: 'Create administrator', exact: true }).click();
    await heading('Overview').waitFor();
    // The browser tab shows only the product name; the page heading names the view.
    assert.equal(await page.title(), 'ShortCircuit');

    // ---------- Create a link ----------
    await newLinkButton.click();
    const dialog = page.getByRole('dialog', { name: 'New link', exact: true });
    const target = 'https://example.test/guide?chapter=intro#start';
    await dialog.getByLabel('Destination URL', { exact: true }).fill(target);
    await dialog.getByLabel('Short link', { exact: true }).fill('test/guide');
    const tagInput = dialog.getByRole('combobox', { name: /^Tags/ });
    for (const name of ['internal', 'docs']) {
      await tagInput.fill(name);
      await tagInput.press('Enter');
      await dialog.getByRole('button', { name: `Remove tag ${name}`, exact: true }).waitFor();
    }
    await dialog.locator('summary').filter({ hasText: 'Title and description' }).click();
    await dialog.getByLabel(/^Title/).fill('Test guide');
    const created = createdLink();
    await dialog.getByRole('button', { name: 'Create link', exact: true }).click();
    const createdResponse = await created;
    assert.equal(createdResponse.status(), 201);
    const link = await createdResponse.json();
    assert.deepEqual(createdResponse.request().postDataJSON().tags, ['internal', 'docs']);
    const done = page.getByRole('dialog', { name: 'Link created', exact: true });
    await done.getByText('test/guide', { exact: true }).waitFor();
    await done.getByRole('img', { name: startsWith('QR code for') }).waitFor();
    await done.getByRole('button', { name: 'Done', exact: true }).click();
    await done.waitFor({ state: 'hidden' });

    // The URL cleaner proposes a URL without tracking parameters, and existing links are found.
    await newLinkButton.click();
    const unclean = target.replace('#start', '&utm_source=sample#start');
    const urlField = dialog.getByLabel('Destination URL', { exact: true });
    await urlField.fill(unclean);
    await dialog.getByText('utm_source', { exact: true }).waitFor();
    await dialog.getByRole('button', { name: 'Parameter and domain options', exact: true }).click();
    const cleaner = dialog.getByRole('region', { name: 'Review URL parameters', exact: true });
    await cleaner.getByText(target, { exact: true }).waitFor();
    await cleaner.getByRole('button', { name: 'Use proposed URL', exact: true }).click();
    assert.equal(await urlField.inputValue(), target);
    // Host and slug are separate flex items, so the accessible name may contain a space between them.
    await dialog.getByRole('link', { name: /^localhost\s*\/test\/guide$/ }).waitFor();
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });

    const specResponse = await page.request.get(admin + '/api/v1/openapi.json');
    assert.equal(specResponse.status(), 200);
    const spec = await specResponse.json();
    assert.equal(spec.openapi, '3.1.0');
    assert.ok(spec.paths['/api/v1/links/{id}/policy'].patch);
    assert.ok(spec.paths['/api/v1/links/{id}'].get);
    assert.equal((await page.request.get(publicOrigin + '/api/v1/openapi.json')).status(), 404);

    const redirect = await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 });
    assert.equal(redirect.status(), 302);
    assert.equal(redirect.headers().location, target);
    assert.equal((await page.request.get(publicOrigin + '/api/v1/auth/session')).status(), 404);

    // One link with its state, limits, protection, traffic, and health fields.
    const single = await page.request.get(admin + '/api/v1/links/' + link.id);
    assert.equal(single.status(), 200);
    const catalogLink = await single.json();
    assert.equal(catalogLink.id, link.id);
    assert.equal(catalogLink.host, 'localhost');
    assert.equal(catalogLink.slug, 'test/guide');
    assert.equal(catalogLink.title, 'Test guide');
    assert.deepEqual([...catalogLink.tags].sort(), ['docs', 'internal']);
    assert.equal(catalogLink.state, 'active');
    assert.equal(catalogLink.starts_at, null);
    assert.equal(catalogLink.expires_at, null);
    assert.equal(catalogLink.max_visits, null);
    assert.equal(typeof catalogLink.visits_used, 'number');
    assert.equal(catalogLink.protection, null);
    assert.equal(catalogLink.rule_count, 0);
    assert.equal(typeof catalogLink.visits_30d, 'number');
    assert.equal(catalogLink.trend.length, 14);
    assert.ok(catalogLink.trend.every(value => Number.isInteger(value) && value >= 0));
    assert.equal(catalogLink.monitor, null);
    assert.equal((await page.request.get(admin + '/api/v1/links/999999')).status(), 404);
    assert.equal((await page.request.get(publicOrigin + '/api/v1/links/' + link.id)).status(), 404);

    // ---------- Quick QR dialog ----------
    await navigate('Links');
    await heading('Links').waitFor();
    await row('localhost/test/guide').getByText('Test guide', { exact: true }).waitFor();
    await row('localhost/test/guide').getByRole('button', { name: 'QR code for test/guide', exact: true }).click();
    const qr = page.getByRole('dialog', { name: 'QR code', exact: true });
    await qr.getByRole('img', { name: startsWith('QR code for') }).waitFor();
    await qr.getByRole('radio', { name: 'PNG 1024', exact: true }).click();
    const downloaded = page.waitForEvent('download');
    await qr.getByRole('button', { name: 'Download', exact: true }).click();
    const qrDownload = await downloaded;
    assert.equal(qrDownload.suggestedFilename(), 'shortcircuit-link-' + link.id + '.png');
    const pngBytes = await readFile(await qrDownload.path());
    assert.equal(pngBytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
    assert.equal(pngBytes.readUInt32BE(16), 1024);
    await toast('QR code downloaded').waitFor();
    const png = await page.request.get(admin + '/api/v1/links/' + link.id + '/qr?format=png&size=512');
    assert.equal(png.status(), 200);
    assert.equal(png.headers()['content-type'], 'image/png');
    await qr.getByRole('radio', { name: 'SVG', exact: true }).click();
    const svgDownloaded = page.waitForEvent('download');
    await qr.getByRole('button', { name: 'Download', exact: true }).click();
    const quickSvg = await readFile(await (await svgDownloaded).path());
    assert.deepEqual(quickSvg, await (await page.request.get(admin + '/api/v1/links/' + link.id + '/qr?format=svg&size=512')).body());
    await qr.getByRole('link', { name: 'Open in QR Studio', exact: true }).click();
    await qr.waitFor({ state: 'hidden' });

    // ---------- QR Studio: styled preview, downloads, templates, logo ----------
    await heading('QR Studio').waitFor();
    assert.match(page.url(), new RegExp(`#/qr/${link.id}$`));
    await page.setViewportSize({ width: 390, height: 844 });
    const design = page.getByRole('region', { name: 'Design', exact: true });
    const preview = page.getByRole('region', { name: 'Preview', exact: true });
    const colorInput = (name) => design.getByLabel(name, { exact: true }).and(page.locator('input[type="color"]'));
    const correction = (level) => design.getByRole('radiogroup', { name: 'Error correction', exact: true })
      .getByRole('radio', { name: startsWith(level) });
    // The segmented control shows the label; the radio inside it sits underneath.
    const chooseCorrection = (level) => design.getByRole('radiogroup', { name: 'Error correction', exact: true })
      .locator('label').filter({ has: page.getByRole('radio', { name: startsWith(level) }) }).click();
    const resolution = page.getByRole('combobox', { name: 'Resolution in pixels', exact: true });
    const downloadButton = page.getByRole('button', { name: 'Download', exact: true });
    await resolution.selectOption('512');
    await colorInput('Foreground color').fill('#002040');
    await colorInput('Background color').fill('#fff4e0');
    await design.getByRole('checkbox', { name: startsWith('Transparent background') }).check();
    await chooseCorrection('H');
    assert.equal(await correction('H').isChecked(), true);
    const quietZone = design.getByRole('slider', { name: startsWith('Quiet zone') });
    await quietZone.evaluate(input => {
      input.value = '8';
      input.dispatchEvent(new Event('change', { bubbles: true }));
    });
    const frame = design.getByRole('checkbox', { name: 'Outline frame', exact: true });
    assert.equal(await frame.isChecked(), false);
    await frame.check();
    const styledPreview = preview.locator('img[src*="frame=outline"]');
    await styledPreview.waitFor({ state: 'visible' });
    const previewUrl = new URL(await styledPreview.getAttribute('src'), admin);
    assert.deepEqual(Object.fromEntries(previewUrl.searchParams), {
      format: 'svg', size: '512', foreground: '002040', background: 'fff4e0',
      transparent: 'true', correction: 'H', quiet_zone: '8', frame: 'outline',
    });
    await noHorizontalScroll(page);
    const styledDownload = page.waitForEvent('download');
    await downloadButton.click();
    const svgDownload = await styledDownload;
    assert.equal(svgDownload.suggestedFilename(), 'shortcircuit-link-' + link.id + '.svg');
    const previewResponse = await page.request.get(previewUrl.href);
    assert.equal(previewResponse.status(), 200);
    const previewBytes = await previewResponse.body();
    assert.deepEqual(await readFile(await svgDownload.path()), previewBytes);
    assert.match(previewBytes.toString('utf8'), /fill="#002040"/);
    assert.doesNotMatch(previewBytes.toString('utf8'), /<rect\b/);
    const copyQr = page.getByRole('button', { name: 'Copy PNG', exact: true });
    if (await copyQr.count()) {
      await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: admin });
      await copyQr.click();
      await toast('QR code copied.').waitFor();
      assert.equal(await page.evaluate(async () => (await navigator.clipboard.read())
        .some(item => item.types.includes('image/png'))), true);
    }
    await design.getByRole('textbox', { name: 'Template name', exact: true }).fill('Print style');
    await design.getByRole('button', { name: 'Save current settings', exact: true }).click();
    await toast('Template saved.').waitFor();
    await colorInput('Foreground color').fill('#eeeeee');
    await page.getByText('Choose a darker foreground and a lighter background for enough contrast.', { exact: true }).waitFor();
    assert.equal(await downloadButton.isDisabled(), true);
    assert.equal(await colorInput('Foreground color').getAttribute('aria-invalid'), 'true');

    // Saved templates survive a reload and apply in the quick QR dialog.
    await page.reload();
    await heading('QR Studio').waitFor();
    const templateTile = design.getByRole('button', { name: 'Print style', exact: true });
    await templateTile.click();
    await styledPreview.waitFor({ state: 'visible' });
    assert.equal(await templateTile.getAttribute('aria-pressed'), 'true');
    assert.equal(await frame.isChecked(), true);
    assert.equal(await styledPreview.getAttribute('src'), previewUrl.pathname + previewUrl.search);
    await noHorizontalScroll(page);
    await page.evaluate(id => { window.location.hash = '#/links/' + id; }, link.id);
    await heading('Test guide').waitFor();
    await page.getByRole('button', { name: 'More actions', exact: true }).click();
    await page.getByRole('menu', { name: 'More actions', exact: true }).getByRole('menuitem', { name: 'QR code', exact: true }).click();
    await qr.getByRole('combobox', { name: 'Design', exact: true }).selectOption({ label: 'Print style' });
    const templatedQuick = qr.locator('img[src*="frame=outline"]');
    await templatedQuick.waitFor({ state: 'visible' });
    assert.match(await templatedQuick.getAttribute('src'), /foreground=002040/);
    await qr.getByRole('button', { name: 'Close', exact: true }).click();
    await qr.waitFor({ state: 'hidden' });
    await page.evaluate(id => { window.location.hash = '#/qr/' + id; }, link.id);
    await heading('QR Studio').waitFor();
    await templateTile.click();
    await styledPreview.waitFor({ state: 'visible' });
    const removeTemplate = design.getByRole('button', { name: 'Delete “Print style”', exact: true });
    const confirmTemplate = page.getByRole('dialog', { name: 'Delete this QR template?', exact: true });
    await removeTemplate.click();
    await confirmTemplate.getByRole('button', { name: 'Cancel', exact: true }).click();
    await confirmTemplate.waitFor({ state: 'hidden' });
    assert.equal(await templateTile.count(), 1);
    await removeTemplate.click();
    await confirmTemplate.getByRole('button', { name: 'Delete template', exact: true }).click();
    await confirmTemplate.waitFor({ state: 'hidden' });
    await toast('Template deleted.').waitFor();
    assert.equal(await templateTile.count(), 0);
    assert.equal(await styledPreview.getAttribute('src'), previewUrl.pathname + previewUrl.search);

    // A logo forces high error correction; a newer change cancels a pending logo preview.
    await chooseCorrection('Q');
    await page.evaluate(() => { window.delayNextLogoPreview = true; });
    await design.getByLabel(startsWith('PNG logo')).setInputFiles({
      name: 'fixture.png',
      mimeType: 'image/png',
      buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAABAAAAAICAYAAADwdn+XAAAAFUlEQVR4nGNQTFv8nxLMMGrAcDAAAC6wFJBQyGgvAAAAAElFTkSuQmCC', 'base64'),
    });
    await page.waitForFunction(() => window.delayedLogoPreview);
    await resolution.selectOption('768');
    await page.waitForFunction(() => window.abortedLogoPreview);
    const logoPreview = preview.locator('[role="img"] canvas:visible');
    await logoPreview.waitFor();
    assert.equal(await logoPreview.evaluate(canvas => canvas.width), 768);
    assert.equal(await correction('H').isChecked(), true);
    assert.equal(await correction('Q').isDisabled(), true);
    assert.deepEqual(await logoPreview.evaluate(canvas =>
      [...canvas.getContext('2d').getImageData(canvas.width / 2, canvas.height / 2, 1, 1).data]), [33, 102, 163, 255]);
    const logoDownload = page.waitForEvent('download');
    await downloadButton.click();
    assert.match(await readFile(await (await logoDownload).path(), 'utf8'), /fill="#2166a3"/);
    await design.getByRole('button', { name: 'Remove logo', exact: true }).click();
    await logoPreview.waitFor({ state: 'hidden' });
    await preview.locator('img[src*="frame=outline"]').waitFor({ state: 'visible' });
    await page.setViewportSize({ width: 1360, height: 900 });

    // ---------- Edit link and link history ----------
    await navigate('Links');
    await heading('Links').waitFor();
    await row('localhost/test/guide').getByRole('button', { name: 'Edit test/guide', exact: true }).click();
    const edit = page.getByRole('dialog', { name: 'Edit link', exact: true });
    await edit.getByLabel('Destination URL', { exact: true }).fill('https://example.test/new-guide');
    await edit.getByLabel(/^Title/).fill('Updated guide');
    await edit.getByRole('button', { name: 'Save changes', exact: true }).click();
    await edit.waitFor({ state: 'hidden' });
    await toast('Link updated.').waitFor();
    await row('localhost/test/guide').getByText('Updated guide', { exact: true }).waitFor();
    await page.setViewportSize({ width: 390, height: 844 });
    await row('localhost/test/guide').getByRole('link', { name: 'Updated guide', exact: true }).click();
    await heading('Updated guide').waitFor();
    await page.getByRole('tab', { name: 'History', exact: true }).click();
    await page.waitForURL(/\?tab=history$/);
    // The tab survives a reload.
    await page.reload();
    const history = page.getByRole('region', { name: 'Link history', exact: true });
    await history.getByRole('button', { name: 'Compare', exact: true }).click();
    await history.locator('del', { hasText: target }).waitFor();
    await history.locator('ins', { hasText: 'https://example.test/new-guide' }).waitFor();
    await noHorizontalScroll(page);
    const exportHistory = page.waitForEvent('download');
    await history.getByRole('button', { name: 'Export history', exact: true }).click();
    const historyFile = await exportHistory;
    assert.equal(historyFile.suggestedFilename(), `shortcircuit-link-${link.id}-history.json`);
    const historyJson = JSON.parse(await readFile(await historyFile.path(), 'utf8'));
    assert.equal(historyJson.items[0].previous.value.target, target);
    await history.getByRole('button', { name: 'Restore', exact: true }).click();
    const restoreRevision = page.getByRole('dialog', { name: 'Restore this revision?', exact: true });
    await restoreRevision.getByRole('button', { name: 'Restore', exact: true }).click();
    await restoreRevision.waitFor({ state: 'hidden' });
    await toast('Revision restored.').waitFor();
    await history.getByRole('button', { name: 'Clear history', exact: true }).click();
    const clearHistory = page.getByRole('dialog', { name: 'Clear link history?', exact: true });
    await clearHistory.getByRole('button', { name: 'Clear history', exact: true }).click();
    await clearHistory.waitFor({ state: 'hidden' });
    await history.getByText('No changes recorded yet.', { exact: true }).waitFor();
    await heading('Test guide').waitFor();
    await page.setViewportSize({ width: 1360, height: 900 });
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).headers().location, target);

    // ---------- Redirect rules and the request simulator ----------
    await page.getByRole('tab', { name: 'Rules', exact: true }).click();
    await page.getByRole('button', { name: 'Add rule', exact: true }).click();
    const firstRule = page.getByRole('group', { name: 'Rule 1', exact: true });
    await firstRule.getByRole('combobox', { name: 'Condition type', exact: true }).selectOption('query');
    await firstRule.getByLabel('Parameter name', { exact: true }).fill('team');
    await firstRule.getByLabel('Match value', { exact: true }).fill('ops');
    await firstRule.getByLabel('Go to', { exact: true }).fill('https://example.test/simulator');
    const simulator = page.getByRole('region', { name: 'Test a request', exact: true });
    await simulator.getByLabel('Query string', { exact: true }).fill('team=ops');
    await simulator.getByRole('button', { name: 'Simulate redirect', exact: true }).click();
    await simulator.getByText('https://example.test/simulator', { exact: true }).waitFor();
    await simulator.getByText('Rule 1 matches · HTTP 302', { exact: true }).waitFor();
    await simulator.getByLabel('Query string', { exact: true }).fill('team=other');
    await simulator.getByText('https://example.test/simulator', { exact: true }).waitFor({ state: 'hidden' });
    const policyPatch = () => page.waitForResponse(response =>
      response.url().endsWith(`/api/v1/links/${link.id}/policy`) && response.request().method() === 'PATCH');
    let rulesSaved = policyPatch();
    await page.getByRole('button', { name: 'Save rules', exact: true }).click();
    assert.equal((await rulesSaved).status(), 200);
    await toast('Rules saved').waitFor();
    const ruled = await page.request.get(publicOrigin + '/test/guide?team=ops', { maxRedirects: 0 });
    assert.equal(ruled.status(), 302);
    assert.equal(ruled.headers().location, 'https://example.test/simulator');
    assert.equal((await (await page.request.get(admin + '/api/v1/links/' + link.id)).json()).rule_count, 1);
    await page.getByRole('button', { name: 'Remove rule 1', exact: true }).click();
    rulesSaved = policyPatch();
    await page.getByRole('button', { name: 'Save rules', exact: true }).click();
    assert.deepEqual((await (await rulesSaved).json()).rules, []);
    assert.equal((await (await page.request.get(admin + '/api/v1/links/' + link.id)).json()).rule_count, 0);

    // ---------- Visit limit and search exclusion ----------
    await page.getByRole('button', { name: 'Edit', exact: true }).first().click();
    await edit.locator('summary').filter({ hasText: 'Schedule and limits' }).click();
    await edit.getByLabel(/^Visit limit/).fill('1');
    await edit.locator('summary').filter({ hasText: 'Redirect behavior' }).click();
    await edit.getByRole('checkbox', { name: startsWith('Request exclusion from search') }).check();
    const policySaved = policyPatch();
    await edit.getByRole('button', { name: 'Save changes', exact: true }).click();
    assert.equal((await policySaved).status(), 200);
    await edit.waitFor({ state: 'hidden' });
    const headGuide = await page.request.head(publicOrigin + '/test/guide', { maxRedirects: 0 });
    assert.equal(headGuide.status(), 302);
    assert.equal(headGuide.headers()['x-robots-tag'], 'noindex, nofollow');
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 302);
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 404);
    const exhausted = await (await page.request.get(admin + '/api/v1/links/' + link.id)).json();
    assert.equal(exhausted.state, 'exhausted');
    assert.equal(exhausted.max_visits, 1);
    assert.equal(exhausted.visits_used, 1);
    await page.evaluate(id => { window.location.hash = `#/links/${id}?tab=access`; }, link.id);
    await page.reload();
    const meter = page.getByRole('meter', { name: 'Visit limit', exact: true });
    await meter.waitFor();
    assert.equal(await meter.getAttribute('aria-valuenow'), '1');
    assert.equal(await meter.getAttribute('aria-valuemax'), '1');
    await page.getByText('The limit is reached. The link no longer redirects.', { exact: true }).waitFor();

    // ---------- Protected and duplicated links ----------
    const protectedTarget = admin + '/readyz';
    const linkPassword = 'ci-browser-link-password';
    await newLinkButton.click();
    await dialog.getByLabel('Destination URL', { exact: true }).fill(protectedTarget);
    await dialog.getByLabel('Short link', { exact: true }).fill('protected-test');
    await dialog.locator('summary').filter({ hasText: 'Access' }).click();
    await dialog.getByRole('radio', { name: startsWith('Password') }).check();
    await dialog.getByLabel('Password', { exact: true }).fill(linkPassword);
    const protectedCreated = createdLink();
    await dialog.getByRole('button', { name: 'Create link', exact: true }).click();
    const protectedResponse = await protectedCreated;
    assert.equal(protectedResponse.status(), 201);
    assert.deepEqual(protectedResponse.request().postDataJSON().protection, { one_time: false, password: linkPassword });
    const protectedLink = await protectedResponse.json();
    assert.ok(!JSON.stringify(protectedLink).includes(linkPassword));
    await done.getByRole('button', { name: 'Done', exact: true }).click();
    await done.waitFor({ state: 'hidden' });
    assert.equal((await (await page.request.get(admin + '/api/v1/links/' + protectedLink.id)).json()).protection, 'password');

    await navigate('Links');
    await heading('Links').waitFor();
    await rowAction('protected-test', 'Duplicate link');
    const duplicate = page.getByRole('dialog', { name: 'Duplicate link', exact: true });
    assert.equal(await duplicate.getByLabel('Destination URL', { exact: true }).inputValue(), protectedTarget);
    assert.equal(await duplicate.getByLabel('Short link', { exact: true }).inputValue(), '');
    await duplicate.locator('summary').filter({ hasText: 'Title and description' }).click();
    await duplicate.getByLabel(/^Description/).fill('Reviewed test description');
    const duplicateCreated = createdLink();
    await duplicate.getByRole('button', { name: 'Create duplicate', exact: true }).click();
    const duplicateResponse = await duplicateCreated;
    assert.equal(duplicateResponse.status(), 201);
    assert.equal(duplicateResponse.request().postDataJSON().protection, undefined);
    assert.equal(duplicateResponse.request().postDataJSON().slug, undefined);
    const copied = await duplicateResponse.json();
    assert.equal(copied.enabled, false);
    assert.equal(copied.description, 'Reviewed test description');
    assert.match(copied.slug, /^[a-zA-Z0-9_-]{10}$/);
    await page.getByRole('dialog', { name: 'Duplicate created', exact: true })
      .getByRole('button', { name: 'Done', exact: true }).click();
    assert.equal((await page.request.get(publicOrigin + '/' + copied.slug, { maxRedirects: 0 })).status(), 404);
    await row('localhost/' + copied.slug).waitFor();
    await rowAction(copied.slug, 'Delete link');
    const deleteOne = page.getByRole('dialog', { name: 'Delete this link?', exact: true });
    await deleteOne.getByRole('button', { name: 'Delete link', exact: true }).click();
    await deleteOne.waitFor({ state: 'hidden' });
    await toast('Link deleted.').waitFor();
    await row('localhost/' + copied.slug).waitFor({ state: 'detached' });

    await row('localhost/protected-test').getByRole('button', { name: 'Edit protected-test', exact: true }).click();
    const accessSection = edit.locator('summary').filter({ hasText: 'Access' });
    await accessSection.click();
    await edit.getByText('Password protection is enabled.', { exact: true }).waitFor();
    assert.equal(await edit.getByLabel(/^New password/).inputValue(), '');
    await edit.getByRole('radio', { name: startsWith('One-time') }).check();
    const protectionSaved = page.waitForResponse(response =>
      response.url().endsWith(`/api/v1/links/${protectedLink.id}/protection`) && response.request().method() === 'PATCH');
    await edit.getByRole('button', { name: 'Save changes', exact: true }).click();
    const protectionResponse = await protectionSaved;
    assert.equal(protectionResponse.status(), 200);
    assert.deepEqual(protectionResponse.request().postDataJSON(), { one_time: true });
    assert.deepEqual(await protectionResponse.json(), { password_required: true, one_time: true, consumed: false });
    await edit.waitFor({ state: 'hidden' });

    const visitor = await browser.newPage();
    const visitorErrors = [];
    watch(visitor, visitorErrors);
    try {
      const shortUrl = publicOrigin + '/protected-test';
      assert.equal((await visitor.goto(shortUrl)).status(), 200);
      await visitor.getByRole('heading', { name: 'This link is protected', exact: true }).waitFor();
      await visitor.getByLabel('Password', { exact: true }).fill(linkPassword);
      const [destination] = await Promise.all([
        visitor.waitForRequest(request => request.url() === protectedTarget),
        visitor.waitForURL(protectedTarget),
        visitor.getByRole('button', { name: 'Continue', exact: true }).click(),
      ]);
      assert.equal(destination.method(), 'GET');
      assert.equal(destination.postData(), null);
      // A used one-time link and an unknown slug show the branded not-found page.
      for (const missing of [shortUrl, publicOrigin + '/no-such-link']) {
        const response = await visitor.goto(missing);
        assert.equal(response.status(), 404);
        assert.match(response.headers()['content-type'], /^text\/html/);
        assert.match(response.headers()['content-security-policy'], /default-src 'none'/);
        assert.doesNotMatch(response.headers()['content-security-policy'], /script-src/);
        await visitor.getByRole('heading', { name: 'This short link doesn’t exist', exact: true }).waitFor();
        assert.equal(await visitor.locator('html').getAttribute('lang'), 'en');
      }
      const german = await visitor.request.get(publicOrigin + '/no-such-link', { headers: { 'Accept-Language': 'de-DE,de;q=0.9' } });
      assert.equal(german.status(), 404);
      assert.match(await german.text(), /Diesen Kurzlink gibt es nicht/);
      const bare = await visitor.request.head(publicOrigin + '/no-such-link');
      assert.equal(bare.status(), 404);
    } finally {
      await visitor.close();
    }
    assert.deepEqual(visitorErrors, []);

    await row('localhost/protected-test').getByRole('button', { name: 'Edit protected-test', exact: true }).click();
    await accessSection.click();
    await edit.getByText('This one-time link has already been used.', { exact: true }).waitFor();
    assert.equal(await edit.getByRole('radiogroup', { name: 'Access', exact: true }).count(), 0);
    await accessSection.getByText('One-time link used', { exact: true }).waitFor();
    await edit.getByRole('button', { name: 'Cancel', exact: true }).click();
    await edit.waitFor({ state: 'hidden' });
    await page.setViewportSize({ width: 390, height: 844 });
    await noHorizontalScroll(page);
    await page.setViewportSize({ width: 1360, height: 900 });

    // ---------- Analytics and privacy controls ----------
    await navigate('Analytics');
    await heading('Analytics').waitFor();
    await page.getByRole('region', { name: 'Key figures', exact: true }).waitFor();
    await navigate('Settings');
    await heading('Settings').waitFor();
    await page.getByRole('navigation', { name: 'Settings sections', exact: true })
      .getByRole('button', { name: 'Privacy and analytics', exact: true }).click();
    await page.waitForURL(/#\/settings\?section=privacy$/);
    const dnt = page.getByRole('checkbox', { name: 'Honor Do Not Track', exact: true });
    const savebar = page.getByRole('region', { name: 'Unsaved changes', exact: true });
    const honored = await dnt.isChecked();
    assert.equal((await (await page.request.get(admin + '/api/v1/analytics/settings')).json()).honor_dnt, honored);
    // Change the setting, save it, and change it back.
    for (const value of [!honored, honored]) {
      await dnt.setChecked(value);
      await savebar.getByText('Unsaved changes', { exact: true }).waitFor();
      const settingsSaved = page.waitForResponse(response =>
        response.url().endsWith('/api/v1/analytics/settings') && response.request().method() === 'PATCH');
      await savebar.getByRole('button', { name: 'Save changes', exact: true }).click();
      assert.equal((await (await settingsSaved).json()).honor_dnt, value);
      await toast('Analytics settings saved.').waitFor();
      await savebar.waitFor({ state: 'hidden' });
      assert.equal((await (await page.request.get(admin + '/api/v1/analytics/settings')).json()).honor_dnt, value);
    }
    await page.getByRole('button', { name: 'Clear analytics…', exact: true }).click();
    const clear = page.getByRole('dialog', { name: 'Clear all analytics?', exact: true });
    const clearButton = clear.getByRole('button', { name: 'Clear analytics data', exact: true });
    assert.equal(await clearButton.isDisabled(), true);
    await clear.getByRole('textbox', { name: 'Confirmation', exact: true }).fill('clear analytics');
    const cleared = page.waitForResponse(response =>
      response.url().endsWith('/api/v1/analytics/data') && response.request().method() === 'DELETE');
    await clearButton.click();
    assert.equal((await cleared).status(), 204);
    await toast('Analytics data cleared.').waitFor();

    // ---------- JSON and CSV exchange ----------
    await navigate('Import and export');
    await heading('Import and export').waitFor();
    const dropZone = page.getByRole('button', { name: startsWith('Choose a file or drop it here') });
    const importApplied = page.getByRole('heading', { name: 'Import applied.', exact: true });
    const importAnother = page.getByRole('button', { name: 'Import another file', exact: true });
    await chooseFile(dropZone, {
      name: 'links.json',
      mimeType: 'application/json',
      buffer: Buffer.from(JSON.stringify({ version: 1, items: [{
        host: 'localhost',
        slug: 'backup-check',
        target: 'https://example.test/backup-check',
        title: 'Backup check',
        description: 'Fixture from native exchange',
        tags: ['internal'],
        enabled: true,
        analytics_enabled: true,
      }] })),
    });
    const previewTable = page.getByRole('table', { name: 'Preview results', exact: true });
    await previewTable.getByText('Will create', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Apply import', exact: true }).click();
    await importApplied.waitFor();
    await page.getByRole('table', { name: 'Import results', exact: true }).getByText('localhost/backup-check', { exact: true }).waitFor();
    assert.equal((await page.request.get(publicOrigin + '/backup-check', { maxRedirects: 0 })).status(), 302);
    await importAnother.click();

    await page.getByRole('radiogroup', { name: 'Export format', exact: true }).getByRole('radio', { name: 'CSV', exact: true }).click();
    const csvDownload = page.waitForEvent('download');
    await page.getByRole('button', { name: 'Download CSV page', exact: true }).click();
    const csvText = await readFile(await (await csvDownload).path(), 'utf8');
    assert.match(csvText, /^version,host,slug,target,title,tags,enabled,analytics_enabled/);

    await chooseFile(dropZone, {
      name: 'links.csv',
      mimeType: 'text/csv',
      buffer: Buffer.from(
        'version,host,slug,target,title,tags,enabled,analytics_enabled\n' +
        '1,localhost,csv-check,https://example.test/csv-check,"Sample, title",[],true,true\n',
      ),
    });
    await previewTable.getByText('localhost/csv-check', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Apply import', exact: true }).click();
    await importApplied.waitFor();
    const csvRedirect = await page.request.get(publicOrigin + '/csv-check', { maxRedirects: 0 });
    assert.equal(csvRedirect.headers().location, 'https://example.test/csv-check');
    await importAnother.click();

    // A rejected import names its first failing row and field.
    await chooseFile(dropZone, {
      name: 'invalid.csv',
      mimeType: 'text/csv',
      buffer: Buffer.from(
        'version,host,slug,target,title,tags,enabled,analytics_enabled\n' +
        '1,localhost,row-one,https://example.test/row-one,One,[],true,true\n' +
        '1,localhost,row-two,https://example.test/row-two,Two,[],maybe,true\n',
      ),
    });
    await page.getByText('Row 2 after the header, field “Enabled”', { exact: true }).waitFor();
    await chooseFile(dropZone, {
      name: 'invalid.json',
      mimeType: 'application/json',
      buffer: Buffer.from(JSON.stringify({ version: 1, items: [
        { host: 'localhost', slug: 'item-one', target: 'https://example.test/one', title: '', tags: [], enabled: true, analytics_enabled: true },
        { host: 'localhost', slug: 'item two', target: 'https://example.test/two', title: '', tags: [], enabled: true, analytics_enabled: true },
      ] })),
    });
    await page.getByText(/^Item 2, field “Slug/).waitFor();
    assert.equal((await page.request.get(publicOrigin + '/row-one', { maxRedirects: 0 })).status(), 404);

    // ---------- Catalog search, filters, views, and organization ----------
    await page.setViewportSize({ width: 768, height: 1024 });
    await navigate('Links');
    await heading('Links').waitFor();
    await noHorizontalScroll(page);
    // Every column shows on a wide screen; narrower containers drop the trend, last visit, tags, and destination.
    await page.setViewportSize({ width: 1920, height: 1080 });
    for (const column of ['Link', 'Destination', 'Tags', 'Visits, 30 days', 'Status', 'Health', 'Last visit']) {
      await linkTable.getByRole('columnheader', { name: column, exact: true }).waitFor();
    }
    // The traffic headers sort through the search API, highest first, and keep the sort in the URL.
    // Few requests here: the whole run stays within the default per-client admin API budget.
    const searched = (match) => page.waitForResponse(response => response.url() === admin + '/api/v1/links/search'
      && match(response.request().postDataJSON() ?? {}));
    const visitsHeader = linkTable.getByRole('columnheader', { name: 'Visits, 30 days', exact: true });
    const lastHeader = linkTable.getByRole('columnheader', { name: 'Last visit', exact: true });
    const sortedSearch = searched(body => body.sort === 'visits_desc');
    await visitsHeader.getByRole('button').click();
    const byVisits = await sortedSearch;
    assert.equal(byVisits.status(), 200);
    await page.waitForURL(/sort=visits_desc/);
    assert.equal(await visitsHeader.getAttribute('aria-sort'), 'descending');
    const visitCounts = (await byVisits.json()).items.map(item => item.visits_30d);
    const counted = visitCounts.filter(count => count !== null);
    assert.ok(counted.length > 0 && visitCounts.slice(0, counted.length).every(count => count !== null));
    assert.deepEqual(counted, [...counted].sort((a, b) => b - a));
    assert.equal(await lastHeader.getAttribute('aria-sort'), null);
    // The status filter uses the live state: the guide link has reached its visit limit.
    const statusFilter = page.getByRole('combobox', { name: 'Filter by status', exact: true });
    assert.deepEqual(await statusFilter.locator('option').allTextContents(),
      ['Any status', 'Active', 'Scheduled', 'Expired', 'Disabled', 'Limit reached']);
    const stateSearch = searched(body => body.state === 'exhausted' && body.sort === 'visits_desc');
    await statusFilter.selectOption({ label: 'Limit reached' });
    const exhaustedPage = await stateSearch;
    assert.equal(exhaustedPage.status(), 200);
    assert.ok((await exhaustedPage.json()).items.every(item => item.state === 'exhausted'));
    await page.waitForURL(/status=exhausted/);
    await row('localhost/test/guide').getByText('Limit reached', { exact: true }).waitFor();
    await row('localhost/csv-check').waitFor({ state: 'detached' });
    await statusFilter.selectOption({ label: 'Any status' });
    await row('localhost/csv-check').waitFor();
    await page.getByRole('button', { name: 'Most visits, 30 days', exact: true }).click();
    await page.getByRole('menu', { name: 'Sort links', exact: true }).getByRole('menuitemradio', { name: 'Newest first', exact: true }).click();
    await page.waitForURL(url => !/sort=|status=/.test(url.hash));
    await page.setViewportSize({ width: 390, height: 844 });
    const filtersToggle = page.getByRole('button', { name: 'Filters', exact: true });
    await filtersToggle.click();
    assert.equal(await filtersToggle.getAttribute('aria-expanded'), 'true');
    await page.getByRole('button', { name: 'Newest first', exact: true }).click();
    await page.getByRole('menu', { name: 'Sort links', exact: true }).getByRole('menuitemradio', { name: 'Slug A–Z', exact: true }).click();
    await page.waitForURL(/sort=slug_asc/);
    const firstShortUrl = linkTable.locator('tbody tr .short-url').first();
    await firstShortUrl.filter({ hasText: 'localhost/backup-check' }).waitFor();
    assert.equal(await firstShortUrl.textContent(), 'localhost/backup-check');
    await page.getByRole('button', { name: 'Tags', exact: true }).click();
    const tagMenu = page.getByRole('menu', { name: 'Filter by tags', exact: true });
    await tagMenu.getByRole('menuitemcheckbox', { name: startsWith('docs') }).click();
    await tagMenu.getByRole('menuitemcheckbox', { name: startsWith('internal') }).click();
    const backupRow = row('localhost/backup-check');
    await page.waitForURL(/tags=docs%2Cinternal|tags=docs,internal/);
    await row('localhost/test/guide').waitFor();
    await backupRow.waitFor({ state: 'detached' });
    assert.equal(await tagMenu.getByRole('menuitemradio', { name: 'Match all tags', exact: true }).getAttribute('aria-checked'), 'true');
    await tagMenu.getByRole('menuitemradio', { name: 'Match any tag', exact: true }).click();
    await backupRow.waitFor();
    await row('localhost/test/guide').waitFor();
    await tagMenu.getByRole('menuitemcheckbox', { name: startsWith('docs') }).click();
    await tagMenu.getByRole('menuitemcheckbox', { name: startsWith('internal') }).click();
    await page.keyboard.press('Escape');
    await tagMenu.waitFor({ state: 'hidden' });
    // The search covers descriptions.
    const searchBox = page.getByRole('searchbox', { name: 'Filter links', exact: true });
    await searchBox.fill('native exchange');
    await backupRow.waitFor();
    assert.equal(await linkTable.locator('tbody tr').count(), 1);
    await searchBox.fill('');
    await row('localhost/csv-check').waitFor();

    const views = page.getByRole('radiogroup', { name: 'Link view', exact: true });
    const view = (name) => views.getByRole('radio', { name: startsWith(name) });
    await rowAction('backup-check', 'Add to favorites');
    await toast('Link added to favorites.').waitFor();
    await view('Favorites').click();
    await backupRow.getByRole('img', { name: 'Favorite', exact: true }).waitFor();
    assert.equal(await linkTable.locator('tbody tr').count(), 1);
    const organizationPath = /\/api\/v1\/links\/\d+\/organization$/;
    let releaseArchive;
    const archivePending = new Promise(resolve => { releaseArchive = resolve; });
    await page.route(organizationPath, async route => {
      await archivePending;
      await route.continue();
    }, { times: 1 });
    const archiveSent = page.waitForRequest(organizationPath);
    await rowAction('backup-check', 'Archive link');
    await archiveSent;
    try {
      await searchBox.focus();
    } finally {
      releaseArchive();
    }
    await page.getByText('There are no favorites.', { exact: true }).waitFor();
    await toast('Link archived.').waitFor();
    // Focus that moved elsewhere while the request ran stays there.
    assert.equal(await searchBox.evaluate(input => input === document.activeElement), true);
    const archivedRedirect = await page.request.get(publicOrigin + '/backup-check', { maxRedirects: 0 });
    assert.equal(archivedRedirect.status(), 302);
    assert.equal(archivedRedirect.headers().location, 'https://example.test/backup-check');
    await view('Archived').click();
    await rowAction('backup-check', 'Unarchive link');
    await page.getByText('There are no archived links.', { exact: true }).waitFor();
    await view('Favorites').click();
    await backupRow.getByRole('img', { name: 'Favorite', exact: true }).waitFor();
    const backupHref = await backupRow.getByRole('link', { name: 'Favorite Backup check', exact: true }).getAttribute('href');
    const backupId = Number(backupHref.match(/^#\/links\/(\d+)$/)[1]);
    await view('All').click();
    await page.getByRole('combobox', { name: 'Filter by domain', exact: true }).selectOption('localhost');
    const searchResponse = page.waitForResponse(response => response.url() === admin + '/api/v1/links/search'
      && response.request().postDataJSON()?.query === 'chapter=intro#start');
    await searchBox.fill('chapter=intro#start');
    const found = await searchResponse;
    assert.equal(found.status(), 200);
    assert.equal(found.request().method(), 'POST');
    assert.deepEqual(found.request().postDataJSON(), { query: 'chapter=intro#start', host: 'localhost', view: 'unarchived', sort: 'slug_asc' });
    await row('localhost/test/guide').waitFor();
    assert.equal(await linkTable.locator('tbody tr').count(), 1);
    await noHorizontalScroll(page);

    // A failed refresh keeps the rows, explains the problem, and recovers with Try again.
    const searchFailure = page.getByRole('alert').filter({ hasText: 'The service is unavailable' });
    const failNextSearch = () => page.route(admin + '/api/v1/links/search',
      route => route.fulfill({ status: 503, json: { code: 'service_unavailable' } }), { times: 1 });
    await failNextSearch();
    await rowAction('test/guide', 'Disable');
    await searchFailure.waitFor();
    await row('localhost/test/guide').getByText('Disabled', { exact: true }).waitFor();
    const recoveredSearch = page.waitForResponse(response =>
      response.url() === admin + '/api/v1/links/search' && response.status() === 200);
    await searchFailure.getByRole('button', { name: 'Try again', exact: true }).click();
    await recoveredSearch;
    await searchFailure.waitFor({ state: 'hidden' });
    await row('localhost/test/guide').getByText('Disabled', { exact: true }).waitFor();

    await failNextSearch();
    await rowAction('test/guide', 'Delete link');
    await deleteOne.getByRole('button', { name: 'Delete link', exact: true }).click();
    await searchFailure.waitFor();
    assert.equal(await linkTable.count(), 0);
    await page.getByRole('button', { name: 'Export', exact: true }).click();
    assert.equal(await page.getByRole('menuitem', { name: startsWith('Loaded links as JSON') }).isDisabled(), true);
    await page.keyboard.press('Escape');
    await searchBox.fill('');
    await page.setViewportSize({ width: 1360, height: 900 });

    // ---------- Bulk actions ----------
    const selectProtected = linkTable.getByRole('checkbox', { name: 'Select localhost/protected-test', exact: true });
    const selectCsv = linkTable.getByRole('checkbox', { name: 'Select localhost/csv-check', exact: true });
    await selectProtected.check();
    await selectCsv.check();
    const bulk = page.getByRole('toolbar', { name: 'Bulk actions', exact: true });
    await bulk.getByText('2 selected', { exact: true }).waitFor();
    await bulk.getByRole('button', { name: 'Tags', exact: true }).click();
    await page.getByRole('menuitem', { name: 'Add tags to selected', exact: true }).click();
    const tagDialog = page.getByRole('dialog', { name: 'Add tags to selected', exact: true });
    const batchTags = tagDialog.getByRole('combobox', { name: 'Tags', exact: true });
    const longTag = 'ä'.repeat(20) + '🟩'.repeat(18) + '00';
    await batchTags.fill(longTag);
    await batchTags.press('Enter');
    await tagDialog.getByRole('button', { name: `Remove tag ${longTag}`, exact: true }).waitFor();
    await page.setViewportSize({ width: 390, height: 844 });
    await noHorizontalScroll(page);
    assert.equal(await tagDialog.evaluate(element => element.scrollWidth > element.clientWidth), false);
    await page.setViewportSize({ width: 1360, height: 900 });
    await tagDialog.getByRole('button', { name: `Remove tag ${longTag}`, exact: true }).click();
    await batchTags.fill('batch-review');
    await batchTags.press('Enter');
    await tagDialog.getByRole('button', { name: 'Apply to selection', exact: true }).click();
    await toast('Selected links updated.').waitFor();
    await tagDialog.waitFor({ state: 'hidden' });
    await linkTable.getByRole('button', { name: 'batch-review', exact: true }).first().waitFor();
    assert.equal(await linkTable.getByRole('button', { name: 'batch-review', exact: true }).count(), 2);
    await selectProtected.check();
    await selectCsv.check();
    await bulk.getByRole('button', { name: 'Export', exact: true }).click();
    const batchDownload = page.waitForEvent('download');
    await page.getByRole('menuitem', { name: 'Export selected (JSON)', exact: true }).click();
    const selectedFile = JSON.parse(await readFile(await (await batchDownload).path(), 'utf8'));
    assert.equal(selectedFile.version, 1);
    assert.deepEqual(selectedFile.items.map(item => [item.slug, item.enabled]).sort(), [['csv-check', true], ['protected-test', false]]);
    await bulk.getByRole('button', { name: 'Delete', exact: true }).click();
    const confirmBulk = page.getByRole('dialog', { name: 'Delete selected links?', exact: true });
    await confirmBulk.getByText('localhost/csv-check', { exact: true }).waitFor();
    await confirmBulk.getByRole('button', { name: 'Cancel', exact: true }).click();
    await confirmBulk.waitFor({ state: 'hidden' });
    assert.equal(await selectCsv.isChecked(), true);
    await bulk.getByRole('button', { name: 'Delete', exact: true }).click();
    const deleteSelected = confirmBulk.getByRole('button', { name: 'Delete selected', exact: true });
    assert.equal(await deleteSelected.isDisabled(), true);
    await confirmBulk.getByLabel('Type 2 to confirm', { exact: true }).fill('2');
    await deleteSelected.click();
    await toast('Selected links deleted.').waitFor();
    await selectCsv.waitFor({ state: 'detached' });
    assert.equal(await selectProtected.count(), 0);
    await row('localhost/backup-check').waitFor();

    // ---------- Tags ----------
    await page.getByRole('navigation', { name: 'Links sections', exact: true }).getByRole('link', { name: startsWith('Tags') }).click();
    await heading('Tags').waitFor();
    const tagRow = page.locator('.tags-table tbody tr').filter({ has: page.getByText('internal', { exact: true }) });
    const editTagDetails = tagRow.getByRole('button', { name: 'Edit tag internal', exact: true });
    await editTagDetails.focus();
    await editTagDetails.press('Enter');
    const tagDetails = page.getByRole('dialog', { name: 'Edit tag', exact: true });
    await tagDetails.getByLabel('Pick a custom color', { exact: true }).fill('#2d7aed');
    assert.equal(await tagDetails.getByRole('radio', { name: 'Custom color', exact: true }).isChecked(), true);
    await tagDetails.getByLabel(/^Tag description/).fill('Team links');
    await tagDetails.getByRole('button', { name: 'Save tag', exact: true }).click();
    await tagDetails.waitFor({ state: 'hidden' });
    await toast('Tag saved').waitFor();
    await tagRow.getByText('Team links', { exact: true }).waitFor();
    assert.equal(await editTagDetails.evaluate(button => button === document.activeElement), true);

    await navigate('Links');
    await heading('Links').waitFor();
    const internalTag = row('localhost/backup-check').getByRole('button', { name: 'internal', exact: true });
    await internalTag.waitFor();
    assert.equal(await internalTag.evaluate(tag => tag.style.getPropertyValue('--tag-color')), '#2d7aed');
    await newLinkButton.click();
    const tagField = dialog.getByRole('combobox', { name: /^Tags/ });
    await tagField.fill('other');
    await tagField.press('Enter');
    await tagField.fill('inte');
    // Choose the suggestion with the keyboard, as the combobox is designed: the first option is active.
    const suggestion = dialog.getByRole('listbox', { name: 'Tag suggestions', exact: true })
      .getByRole('option', { name: startsWith('internal') });
    await suggestion.waitFor();
    assert.equal(await suggestion.getAttribute('aria-selected'), 'true');
    assert.equal(await tagField.getAttribute('aria-activedescendant'), await suggestion.getAttribute('id'));
    await tagField.press('Enter');
    await dialog.getByRole('button', { name: 'Remove tag internal', exact: true }).waitFor();
    await dialog.getByRole('button', { name: 'Remove tag other', exact: true }).waitFor();
    assert.equal(await tagField.inputValue(), '');
    assert.equal(await tagField.evaluate(input => input === document.activeElement), true);
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    await noHorizontalScroll(page);

    // ---------- Command palette ----------
    await page.keyboard.press('Control+k');
    const palette = page.getByRole('dialog', { name: 'Search links and pages', exact: true });
    await palette.getByRole('combobox').fill('Backup check');
    await palette.getByRole('option', { name: /Backup check/ }).first().waitFor();
    await palette.getByRole('combobox').press('Enter');
    await heading('Backup check').waitFor();
    assert.match(page.url(), new RegExp(`#/links/${backupId}$`));
    await page.getByText('Fixture from native exchange', { exact: true }).waitFor();

    // ---------- Domains ----------
    await navigate('Domains');
    await heading('Domains').waitFor();
    await page.getByRole('button', { name: 'Edit localhost', exact: true }).click();
    const domainSettings = page.getByRole('dialog', { name: 'Edit domain', exact: true });
    await domainSettings.getByRole('checkbox', { name: startsWith('Allow search engines') }).uncheck();
    await domainSettings.getByRole('button', { name: 'Save domain', exact: true }).click();
    await domainSettings.waitFor({ state: 'hidden' });
    await toast('Domain saved').waitFor();
    const robots = await page.request.get(publicOrigin + '/robots.txt');
    assert.equal(robots.status(), 200);
    assert.equal(await robots.text(), 'User-agent: *\nDisallow: /\n');

    // ---------- Mapped CSV import ----------
    await navigate('Import and export');
    await heading('Import and export').waitFor();
    await chooseFile(dropZone, {
      name: 'mapped.csv',
      mimeType: 'text/csv',
      buffer: Buffer.from('Address,Short code,Status\nhttps://example.test/mapped-check,mapped-check,1\n'),
    });
    const mapping = page.getByRole('group', { name: 'Map CSV columns', exact: true });
    await mapping.getByRole('combobox', { name: startsWith('Destination URL') }).selectOption('0');
    await mapping.getByRole('combobox', { name: startsWith('Slug') }).selectOption('1');
    await mapping.getByRole('combobox', { name: 'Enabled', exact: true }).selectOption('2');
    await mapping.getByRole('combobox', { name: 'Domain for all rows', exact: true }).selectOption('localhost');
    await page.getByRole('button', { name: 'Preview import', exact: true }).click();
    await previewTable.getByText('localhost/mapped-check', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Apply import', exact: true }).click();
    await importApplied.waitFor();
    const mapped = await page.request.get(publicOrigin + '/mapped-check', { maxRedirects: 0 });
    assert.equal(mapped.headers().location, 'https://example.test/mapped-check');

    // ---------- Domains: delete a domain (0.9.0, domains agent) ----------
    // moving.test gets two links, one of which also exists on spare.test. A move to spare.test is
    // blocked by the conflict, so moving.test is deleted with its links after typing its name.
    // spare.test is then deleted by moving its link to localhost, at phone widths.
    await navigate('Domains');
    await heading('Domains').waitFor();
    for (const host of ['moving.test', 'spare.test']) {
      await page.getByRole('button', { name: 'Add domain', exact: true }).click();
      const add = page.getByRole('dialog', { name: 'Add domain', exact: true });
      await add.getByLabel('Host', { exact: true }).fill(host);
      // Leaving the host field suggests the HTTPS origin.
      const origin = add.getByLabel('Public origin', { exact: true });
      await origin.click();
      await page.waitForFunction(([field, value]) => document.getElementById(field)?.value === value, ['dm-origin', `https://${host}`]);
      await add.getByRole('button', { name: 'Add domain', exact: true }).click();
      await add.waitFor({ state: 'hidden' });
      await toast('Domain added').waitFor();
    }
    await navigate('Import and export');
    await heading('Import and export').waitFor();
    if (await importAnother.isVisible()) await importAnother.click();
    await chooseFile(dropZone, {
      name: 'domain-links.json',
      mimeType: 'application/json',
      buffer: Buffer.from(JSON.stringify({ version: 1, items: [
        ['moving.test', 'keep', 'https://example.test/keep'],
        ['moving.test', 'clash', 'https://example.test/clash-a'],
        ['spare.test', 'clash', 'https://example.test/clash-b'],
      ].map(([host, slug, target]) => ({
        host, slug, target, title: '', description: '', tags: [], enabled: true, analytics_enabled: true,
      })) })),
    });
    await previewTable.getByText('Will create', { exact: true }).first().waitFor();
    await page.getByRole('button', { name: 'Apply import', exact: true }).click();
    await importApplied.waitFor();
    await importAnother.click();

    await navigate('Domains');
    await heading('Domains').waitFor();
    const domainAction = async (host, item) => {
      await page.getByRole('button', { name: `More actions for ${host}`, exact: true }).click();
      await page.getByRole('menu', { name: `More actions for ${host}`, exact: true })
        .getByRole('menuitem', { name: item, exact: true }).click();
    };
    await domainAction('moving.test', 'Delete domain');
    const removeDomain = page.getByRole('dialog', { name: 'Delete moving.test', exact: true });
    const removeButton = removeDomain.getByRole('button', { name: 'Delete domain', exact: true });
    await removeDomain.getByText(startsWith('2 links use this domain')).waitFor();
    await removeDomain.getByLabel('Target domain', { exact: true }).selectOption('spare.test');
    await removeDomain.getByText(startsWith('One slug already exists on spare.test')).waitFor();
    await removeDomain.locator('.slug-list').getByText('clash', { exact: true }).waitFor();
    await removeDomain.getByText(startsWith('Short URLs and printed QR codes for moving.test stop working')).waitFor();
    assert.equal(await removeButton.isDisabled(), true);
    await removeDomain.getByRole('radio', { name: startsWith('Delete the links') }).check();
    await removeDomain.getByRole('link', { name: 'Open Import and export', exact: true }).waitFor();
    const typed = removeDomain.getByLabel('Type moving.test to confirm', { exact: true });
    await typed.fill('moving');
    assert.equal(await removeButton.isDisabled(), true);
    await typed.fill('moving.test');
    await removeButton.click();
    await toast('Domain deleted').waitFor();
    await page.locator('.toasts').getByText('moving.test: 2 links deleted', { exact: true }).waitFor();
    await removeDomain.waitFor({ state: 'hidden' });
    await page.waitForFunction(() => document.activeElement?.tagName === 'H1');
    assert.equal(await page.getByRole('button', { name: 'More actions for moving.test', exact: true }).count(), 0);

    // The dialog is a sheet on phones: check it at 390 and 320 px without horizontal scrolling.
    await page.setViewportSize({ width: 390, height: 844 });
    await domainAction('spare.test', 'Delete domain');
    const spare = page.getByRole('dialog', { name: 'Delete spare.test', exact: true });
    await spare.getByText('One link uses this domain. Choose what happens to it.', { exact: true }).waitFor();
    await spare.getByLabel('Target domain', { exact: true }).selectOption('localhost');
    await spare.getByText('No slug conflicts on localhost.', { exact: true }).waitFor();
    await noHorizontalScroll(page);
    await page.setViewportSize({ width: 320, height: 568 });
    await noHorizontalScroll(page);
    await spare.getByRole('button', { name: 'Delete domain', exact: true }).click();
    await page.locator('.toasts').getByText('spare.test: one link moved to localhost', { exact: true }).waitFor();
    await spare.waitFor({ state: 'hidden' });
    await page.setViewportSize({ width: 1360, height: 900 });
    assert.equal((await page.request.get(publicOrigin + '/clash', { maxRedirects: 0 })).headers().location, 'https://example.test/clash-b');
    const domainAudit = (await (await page.request.get(admin + '/api/v1/audit')).json()).items
      .filter(item => item.action === 'domain_delete').map(item => item.details);
    assert.deepEqual(domainAudit, [
      { host: 'spare.test', links: 'move', target: 'localhost', count: 1, environment: false },
      { host: 'moving.test', links: 'delete', target: null, count: 2, environment: false },
    ]);

    // ---------- Monitoring defaults ----------
    await navigate('Health');
    await heading('Health').waitFor();
    await page.getByText('Health checks are off.', { exact: false }).first().waitFor();
    assert.equal(await page.getByRole('button', { name: 'Monitor links', exact: true }).isDisabled(), true);
    const monitor = await page.request.get(admin + '/api/v1/monitoring');
    assert.deepEqual(await monitor.json(), { enabled: false, items: [], next_after: null });
    assert.equal((await page.request.get(publicOrigin + '/api/v1/monitoring')).status(), 404);

    // ---------- Settings > Operations (runtime settings; settings agent) ----------
    // The Health notice links to the settings that replace MONITOR_ENABLED and the other
    // container variables. Switches apply at once, other values save through the savebar, and
    // the webhook needs the password and is never shown in full.
    {
      await page.getByRole('link', { name: 'Settings › Operations', exact: true }).first().click();
      await page.waitForURL(/#\/settings\?section=operations$/);
      const healthPanel = page.getByRole('region', { name: 'Health checks', exact: true });
      const healthSwitch = healthPanel.getByRole('checkbox', { name: 'Allow health checks', exact: true });
      assert.equal(await healthSwitch.isChecked(), false);
      await healthPanel.getByText('Default', { exact: true }).first().waitFor();
      const operationsPatch = () => page.waitForResponse(response =>
        response.url().endsWith('/api/v1/settings/operations') && response.request().method() === 'PATCH');
      let patched = operationsPatch();
      await healthSwitch.check();
      assert.equal((await (await patched).json()).monitor_enabled.source, 'saved');
      await toast('Health checks on').waitFor();
      assert.equal((await (await page.request.get(admin + '/api/v1/monitoring')).json()).enabled, true);
      patched = operationsPatch();
      await healthPanel.getByRole('button', { name: 'Reset to default', exact: true }).click();
      assert.equal((await (await patched).json()).monitor_enabled.source, 'default');
      await toast('Reset to the container value or default').waitFor();
      assert.equal(await healthSwitch.isChecked(), false);

      const backupsPanel = page.getByRole('region', { name: 'Scheduled backups', exact: true });
      await backupsPanel.getByLabel('Backup interval', { exact: true }).fill('12');
      const operationsBar = page.getByRole('region', { name: 'Unsaved changes', exact: true });
      patched = operationsPatch();
      await operationsBar.getByRole('button', { name: 'Save changes', exact: true }).click();
      const savedBackups = await (await patched).json();
      assert.deepEqual(savedBackups.backup_interval_hours, { value: 12, source: 'saved', container: null });
      await toast('Operations settings saved').waitFor();
      await operationsBar.waitFor({ state: 'hidden' });
      patched = operationsPatch();
      await backupsPanel.getByRole('button', { name: 'Reset to default', exact: true }).click();
      assert.equal((await (await patched).json()).backup_interval_hours.value, 24);

      const secret = 'https://hooks.example/services/synthetic-browser-secret';
      const notificationsPanel = page.getByRole('region', { name: 'Notifications', exact: true });
      await notificationsPanel.getByRole('button', { name: 'Change webhook…', exact: true }).click();
      const target = page.getByRole('dialog', { name: 'Notification webhook', exact: true });
      await target.getByLabel('Webhook URL', { exact: true }).fill(secret);
      await target.getByLabel('Current password', { exact: true }).fill(adminPassword);
      patched = operationsPatch();
      await target.getByRole('button', { name: 'Save webhook', exact: true }).click();
      const savedTarget = await patched;
      assert.equal(savedTarget.status(), 200);
      assert.equal(JSON.stringify(await savedTarget.json()).includes('synthetic-browser-secret'), false);
      await toast('Webhook saved').waitFor();
      await target.waitFor({ state: 'hidden' });
      await notificationsPanel.getByText('https://hooks.example', { exact: true }).waitFor();
      assert.equal((await page.content()).includes('synthetic-browser-secret'), false);
      await page.setViewportSize({ width: 390, height: 844 });
      await noHorizontalScroll(page);
      await page.setViewportSize({ width: 1360, height: 900 });
      await notificationsPanel.getByRole('button', { name: 'Reset to default', exact: true }).click();
      const resetTarget = page.getByRole('dialog', { name: 'Use the container webhook?', exact: true });
      await resetTarget.getByLabel('Current password', { exact: true }).fill(adminPassword);
      patched = operationsPatch();
      await resetTarget.getByRole('button', { name: 'Reset to default', exact: true }).click();
      assert.equal((await (await patched).json()).notifications_active, false);
      await resetTarget.waitFor({ state: 'hidden' });
      await notificationsPanel.getByText('Not set', { exact: true }).first().waitFor();
      const settingsAudit = (await (await page.request.get(admin + '/api/v1/audit')).json()).items
        .filter(item => item.action.startsWith('operations_settings'));
      assert.equal(JSON.stringify(settingsAudit).includes('synthetic-browser-secret'), false);
      assert.ok(settingsAudit.some(item => item.action === 'operations_settings_authorize'));
    }

    // ---------- Settings > Public pages (0.9.0, public-pages agent) ----------
    // Fields mode edits the English missing page, the sandboxed preview shows the server's
    // rendering, saving changes the public 404, and reset restores the built-in page.
    {
      await navigate('Settings');
      await heading('Settings').waitFor();
      await page.getByRole('navigation', { name: 'Settings sections', exact: true })
        .getByRole('button', { name: 'Public pages', exact: true }).click();
      await page.waitForURL(/#\/settings\?section=pages$/);
      const editor = page.getByRole('region', { name: 'Public pages', exact: true });
      await editor.getByLabel('Page', { exact: true }).selectOption('missing');
      await editor.getByRole('radiogroup', { name: 'Language', exact: true })
        .getByRole('radio', { name: 'English', exact: true }).click();
      await editor.getByText('Uses the built-in page', { exact: false }).waitFor();
      const customTitle = 'Browser test: this link is unavailable';
      await editor.getByLabel(startsWith('Title')).fill(customTitle);
      const previewed = page.waitForResponse(response =>
        response.url() === admin + '/api/v1/settings/public-pages/preview' && response.request().method() === 'POST');
      await editor.getByRole('button', { name: 'Update preview', exact: true }).click();
      const previewUrl = (await (await previewed).json()).url;
      const frame = editor.locator('iframe.preview-frame');
      assert.equal(await frame.getAttribute('sandbox'), '');
      await page.frameLocator('iframe.preview-frame').getByRole('heading', { name: customTitle, exact: true }).waitFor();
      const framed = await page.request.get(admin + previewUrl);
      assert.match(framed.headers()['content-security-policy'], /frame-ancestors 'self'$/);
      assert.doesNotMatch(framed.headers()['content-security-policy'], /script-src/);
      assert.equal(framed.headers()['x-frame-options'], 'SAMEORIGIN');

      const savedPage = page.waitForResponse(response =>
        response.url() === admin + '/api/v1/settings/public-pages' && response.request().method() === 'PATCH');
      await editor.getByRole('button', { name: 'Save page', exact: true }).click();
      assert.equal((await savedPage).status(), 200);
      await toast('Page saved').waitFor();
      await editor.getByText('Uses custom fields', { exact: false }).waitFor();
      const customized = await page.request.get(publicOrigin + '/no-such-link');
      assert.equal(customized.status(), 404);
      assert.ok((await customized.text()).includes(customTitle));
      assert.match(customized.headers()['content-security-policy'], /img-src data:; form-action 'self'; base-uri 'none'; frame-ancestors 'none'/);
      assert.doesNotMatch(customized.headers()['content-security-policy'], /script-src/);
      await page.setViewportSize({ width: 390, height: 844 });
      await noHorizontalScroll(page);
      await page.setViewportSize({ width: 1360, height: 900 });

      await editor.getByRole('button', { name: 'Reset to built-in', exact: true }).click();
      const resetPage = page.getByRole('dialog', { name: 'Reset “Missing or inactive link” (English) to the built-in page?', exact: true });
      await resetPage.getByRole('button', { name: 'Reset to built-in', exact: true }).click();
      await toast('Built-in page restored').waitFor();
      await editor.getByText('Uses the built-in page', { exact: false }).waitFor();
      assert.ok((await (await page.request.get(publicOrigin + '/no-such-link')).text()).includes('This short link doesn’t exist'));
      const pagesAudit = (await (await page.request.get(admin + '/api/v1/audit')).json()).items
        .filter(item => item.action.startsWith('public_pages')).map(item => item.action);
      assert.deepEqual(pagesAudit, ['public_pages_reset', 'public_pages_update']);
    }

    // ---------- Browser backup, staged restore, and the audit log ----------
    await navigate('System');
    await heading('System').waitFor();
    await page.getByText('CPU sample', { exact: true }).waitFor();
    await page.getByRole('tab', { name: 'Backups', exact: true }).click();
    await page.waitForURL(/#\/system\?tab=backups$/);
    // Stored snapshots are listed read-only by file name, without paths.
    const storedPanel = page.getByRole('region', { name: 'Stored snapshots', exact: true });
    const storedTable = storedPanel.getByRole('table');
    await storedPanel.getByText('No stored snapshots', { exact: true }).or(storedTable).waitFor();
    const storedList = await (await page.request.get(admin + '/api/v1/system/backups')).json();
    assert.ok(Array.isArray(storedList.items));
    assert.equal(JSON.stringify(storedList).includes('/data'), false);
    if (storedList.items.length) {
      await storedTable.getByRole('columnheader', { name: 'Type', exact: true }).waitFor();
      await storedTable.getByText(storedList.items[0].file_name, { exact: true }).waitFor();
    }
    await page.getByRole('button', { name: 'Download backup', exact: true }).click();
    const operations = page.getByRole('dialog', { name: 'Download backup', exact: true });
    await operations.getByLabel('Current password', { exact: true }).fill(adminPassword);
    const backupEvent = page.waitForEvent('download');
    await operations.getByRole('button', { name: 'Download backup', exact: true }).click();
    const backupFile = await backupEvent;
    assert.match(backupFile.suggestedFilename(), /^shortcircuit-schema[0-9]+-[0-9]+\.sqlite$/);
    const backupBytes = await readFile(await backupFile.path());
    assert.equal(backupBytes.subarray(0, 16).toString(), 'SQLite format 3\0');
    await operations.waitFor({ state: 'hidden' });

    await page.getByRole('button', { name: 'Restore from a backup…', exact: true }).click();
    const restore = page.getByRole('dialog', { name: 'Restore from a backup', exact: true });
    const stage = restore.getByRole('button', { name: 'Validate and stage restore', exact: true });
    assert.equal(await stage.isDisabled(), true);
    await chooseFile(restore.getByRole('button', { name: startsWith('SQLite backup file') }), {
      name: backupFile.suggestedFilename(), mimeType: 'application/octet-stream', buffer: backupBytes,
    });
    await restore.getByText(backupFile.suggestedFilename(), { exact: true }).waitFor();
    await restore.getByRole('checkbox').check();
    await restore.getByLabel('Current password', { exact: true }).fill(adminPassword);
    await stage.click();
    await toast('The backup was verified and staged.').waitFor();
    await restore.waitFor({ state: 'hidden' });
    await page.getByText('Pending restart', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Discard pending restore', exact: true }).click();
    const discard = page.getByRole('dialog', { name: 'Discard pending restore', exact: true });
    await discard.getByLabel('Current password', { exact: true }).fill(adminPassword);
    await discard.getByRole('button', { name: 'Discard pending restore', exact: true }).click();
    await toast('Pending restore discarded.').waitFor();
    await discard.waitFor({ state: 'hidden' });

    await page.getByRole('tab', { name: 'Audit log', exact: true }).click();
    await page.reload();
    await page.getByRole('tab', { name: 'Audit log', exact: true, selected: true }).waitFor();
    await page.getByRole('cell', { name: 'Restore discarded', exact: true }).first().waitFor();
    await page.getByRole('cell', { name: 'Backup download', exact: true }).first().waitFor();
    for (const [width, height] of [[1920, 1080], [1440, 900], [1280, 800], [1024, 768], [768, 1024], [390, 844], [320, 568]]) {
      await page.setViewportSize({ width, height });
      await noHorizontalScroll(page);
    }
    await page.setViewportSize({ width: 1360, height: 900 });

    // ---------- Settings: appearance and time zone ----------
    await navigate('Settings');
    await heading('Settings').waitFor();
    await page.getByRole('navigation', { name: 'Settings sections', exact: true })
      .getByRole('button', { name: 'Appearance', exact: true }).click();
    const themes = page.getByRole('radiogroup', { name: 'Theme', exact: true });
    await themes.getByRole('radio', { name: 'Light', exact: true }).click();
    assert.equal(await page.evaluate(() => [document.documentElement.dataset.theme, localStorage.getItem('shortcircuit-theme')].join()), 'light,light');
    await themes.getByRole('radio', { name: 'Dark', exact: true }).click();
    assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    await page.getByLabel('Time zone', { exact: true }).selectOption('UTC');
    await savebar.getByRole('button', { name: 'Save changes', exact: true }).click();
    await page.waitForFunction(() => localStorage.getItem('shortcircuit-timezone') === 'UTC');
    await savebar.waitFor({ state: 'hidden' });

    // ---------- Accessibility in the dark and light theme ----------
    await auditRoutes(browser, await context.storageState(), axeSource, { linkId: backupId });

    // ---------- Sign-out ----------
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByRole('button', { name: 'Open navigation', exact: true }).click();
    await page.getByRole('button', { name: 'Account menu', exact: true }).click();
    await page.getByRole('menuitem', { name: 'Sign out', exact: true }).click();
    await page.getByRole('button', { name: 'Sign in', exact: true }).waitFor();
    await page.setViewportSize({ width: 1360, height: 900 });
    assert.equal((await page.request.get(admin + '/api/v1/links')).status(), 401);
    assert.equal((await page.request.get(admin + '/api/v1/links/' + backupId)).status(), 401);
    assert.equal((await page.request.get(admin + '/api/v1/openapi.json')).status(), 401);
    assert.deepEqual(errors, []);
    console.log('Browser smoke passed: setup, sign-in, favicons, CSP and compressed assets, create link, URL tools, '
      + 'quick QR and QR Studio with templates, downloads and validation, link history, rules and simulator, visit limits, '
      + 'protected links, branded public pages, analytics controls, JSON and CSV exchange, catalog search, bulk actions, '
      + 'tags, domains, monitoring defaults, browser backup and staged restore, auditing, responsive layout, '
      + 'axe WCAG 2.2 AA in both themes, and sign-out.');
  } catch (error) {
    // SHORTCIRCUIT_ARTIFACTS names a directory for a failure screenshot of each open page.
    if (process.env.SHORTCIRCUIT_ARTIFACTS) {
      const pages = browser.contexts().flatMap(context => context.pages());
      await Promise.all(pages.map((open, index) => open.screenshot({
        path: path.join(process.env.SHORTCIRCUIT_ARTIFACTS, `failure-${index}.png`), fullPage: true,
      }).catch(() => {})));
    }
    throw error;
  } finally {
    await browser.close();
  }
}

// Waits until the page shows no loading state, then lets transitions finish.
async function settle(page) {
  await page.waitForLoadState('networkidle').catch(() => {});
  await page.waitForFunction(() => !document.querySelector('[aria-busy="true"]')
    && ![...document.querySelectorAll('[role="status"]')].some(element => /Loading/.test(element.textContent ?? '')),
  undefined, { timeout: 10_000 }).catch(() => {});
  await page.waitForTimeout(300);
}

// Runs axe-core with the WCAG 2.2 A and AA rules. Violations are added to `findings` when given,
// otherwise they fail at once with a readable list.
async function auditPage(page, label, axeSource, findings) {
  await settle(page);
  // Runtime.evaluate is not subject to the page CSP, so axe needs no inline script.
  if (!(await page.evaluate(() => typeof window.axe === 'object'))) await page.evaluate(axeSource);
  const violations = await page.evaluate(async (tags) => {
    const result = await window.axe.run(document, { runOnly: { type: 'tag', values: tags }, resultTypes: ['violations'] });
    return result.violations.map(violation => ({
      id: violation.id,
      impact: violation.impact,
      help: violation.help,
      nodes: violation.nodes.slice(0, 5).map(node => `${node.target.join(' ')}: ${node.failureSummary.replace(/\s+/g, ' ')}`),
    }));
  }, axeTags);
  if (violations.length) {
    const theme = await page.evaluate(() => document.documentElement.dataset.theme ?? 'unknown');
    const message = `Accessibility violations on ${label} (${theme} theme):\n`
      + violations.map(v => `- ${v.id} (${v.impact}): ${v.help}\n    ${v.nodes.join('\n    ')}`).join('\n');
    if (!findings) throw new Error(message);
    findings.push(message);
  }
}

async function auditRoutes(browser, storageState, axeSource, { linkId }) {
  const routes = [
    ['Overview', '#/overview'],
    ['Links', '#/links'],
    ['Archived links', '#/links?view=archived'],
    ['Tags', '#/tags'],
    ['Link overview', `#/links/${linkId}`],
    ['Link analytics', `#/links/${linkId}?tab=analytics`],
    ['Link rules', `#/links/${linkId}?tab=rules`],
    ['Link access', `#/links/${linkId}?tab=access`],
    ['Link health', `#/links/${linkId}?tab=health`],
    ['Link history', `#/links/${linkId}?tab=history`],
    ['Analytics', '#/analytics'],
    ['Health', '#/health'],
    ['QR Studio', `#/qr/${linkId}`],
    ['Domains', '#/domains'],
    ['Destination policy', '#/domains?tab=policy'],
    ['Import and export', '#/exchange'],
    ['API keys', '#/keys'],
    ['System status', '#/system'],
    ['System backups', '#/system?tab=backups'],
    ['System notifications', '#/system?tab=notifications'],
    ['System jobs', '#/system?tab=jobs'],
    ['Audit log', '#/system?tab=audit'],
    ['Account settings', '#/settings'],
    ['Appearance settings', '#/settings?section=appearance'],
    ['Privacy settings', '#/settings?section=privacy'],
    ['Security settings', '#/settings?section=security'],
    ['Operations settings', '#/settings?section=operations'],
    ['Public pages settings', '#/settings?section=pages'],
  ];
  const findings = [];
  for (const theme of ['dark', 'light']) {
    const errors = [];
    const context = await browser.newContext({ storageState, viewport: { width: 1360, height: 900 }, reducedMotion: 'reduce' });
    await paceApi(context);
    await context.addInitScript(value => localStorage.setItem('shortcircuit-theme', value), theme);
    try {
      const page = await context.newPage();
      watch(page, errors);
      for (const [label, hash] of routes) {
        await page.goto(admin + '/' + hash);
        await page.locator('main h1').first().waitFor();
        assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), theme);
        await auditPage(page, label, axeSource, findings);
      }
      // Overlays: the create dialog, the edit drawer, the QR dialog, and the command palette.
      await page.goto(admin + '/#/links');
      await page.locator('header.topbar').getByRole('button', { name: 'New link', exact: true }).click();
      const create = page.getByRole('dialog', { name: 'New link', exact: true });
      for (const section of ['Title and description', 'Schedule and limits', 'Access', 'Redirect behavior', 'Analytics']) {
        await create.locator('summary').filter({ hasText: section }).click();
      }
      await auditPage(page, 'New link dialog', axeSource, findings);
      await create.getByRole('button', { name: 'Cancel', exact: true }).click();
      await page.getByRole('button', { name: 'Edit backup-check', exact: true }).click();
      await page.getByRole('dialog', { name: 'Edit link', exact: true }).getByLabel('Destination URL', { exact: true }).waitFor();
      await auditPage(page, 'Edit link drawer', axeSource, findings);
      await page.keyboard.press('Escape');
      await page.getByRole('button', { name: 'QR code for backup-check', exact: true }).click();
      await page.getByRole('dialog', { name: 'QR code', exact: true }).getByRole('img').waitFor();
      await auditPage(page, 'QR dialog', axeSource, findings);
      await page.keyboard.press('Escape');
      await page.keyboard.press('Control+k');
      await page.getByRole('dialog', { name: 'Search links and pages', exact: true }).waitFor();
      await auditPage(page, 'Command palette', axeSource, findings);
      await page.keyboard.press('Escape');
      await page.goto(admin + '/#/system?tab=backups');
      await page.getByRole('button', { name: 'Restore from a backup…', exact: true }).click();
      await page.getByRole('dialog', { name: 'Restore from a backup', exact: true }).waitFor();
      await auditPage(page, 'Restore dialog', axeSource, findings);
      await page.keyboard.press('Escape');
      // The navigation drawer on a phone.
      await page.setViewportSize({ width: 390, height: 844 });
      await page.goto(admin + '/#/links');
      await page.locator('main h1').first().waitFor();
      await auditPage(page, 'Links on a phone', axeSource, findings);
      await page.getByRole('button', { name: 'Open navigation', exact: true }).click();
      await auditPage(page, 'Navigation drawer', axeSource, findings);
      await page.close();

      // Signed out: the sign-in page. Public: the not-found page.
      const anonymous = await browser.newContext({ viewport: { width: 1360, height: 900 }, reducedMotion: 'reduce' });
      await paceApi(anonymous);
      await anonymous.addInitScript(value => localStorage.setItem('shortcircuit-theme', value), theme);
      try {
        const signIn = await anonymous.newPage();
        watch(signIn, errors);
        await signIn.goto(admin);
        await signIn.getByRole('button', { name: 'Sign in', exact: true }).waitFor();
        await auditPage(signIn, 'Sign-in', axeSource, findings);
        await signIn.goto(publicOrigin + '/no-such-link');
        await auditPage(signIn, 'Public not-found page', axeSource, findings);
      } finally {
        await anonymous.close();
      }
    } finally {
      await context.close();
    }
    assert.deepEqual(errors, [], `${theme} theme`);
  }
  // Report every route at once instead of stopping at the first one.
  if (findings.length) throw new Error(findings.join('\n'));
}

verify().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
