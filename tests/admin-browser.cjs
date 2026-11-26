const assert = require('node:assert/strict');
const { readFile } = require('node:fs/promises');
const { chromium } = require(process.env.SHORTCIRCUIT_PLAYWRIGHT);

const admin = 'http://localhost:8081';
const publicOrigin = 'http://localhost:8080';
const setupToken = process.env.SHORTCIRCUIT_SETUP_TOKEN;

async function verify() {
  assert.match(setupToken, /^[a-f0-9]{64}$/);
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ viewport: { width: 1360, height: 900 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
      if (message.type() === 'error' && /Content Security Policy/i.test(message.text())) {
        errors.push(message.text());
      }
    });
    const pageResponse = await page.goto(admin);
    assert.ok(pageResponse.ok());
    assert.match(pageResponse.headers()['content-security-policy'], /frame-ancestors 'none'/);
    await page.getByLabel('Setup token', { exact: true }).fill(setupToken);
    await page.getByLabel('Username', { exact: true }).fill('Test administrator');
    await page.getByLabel('Password', { exact: true }).fill('ci-browser-password-example');
    await page.getByRole('button', { name: 'Create administrator', exact: true }).click();

    await page.getByRole('button', { name: 'Create link', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Create link', exact: true });
    const target = 'https://example.test/guide?chapter=intro#start';
    await dialog.getByLabel('Destination URL', { exact: true }).fill(target);
    await dialog.getByLabel('Title (optional)', { exact: true }).fill('Test guide');
    await dialog.getByLabel('Slug (optional)', { exact: true }).fill('test/guide');
    await dialog.getByLabel('Tags', { exact: true }).fill('internal, docs');
    const created = page.waitForResponse(response =>
      response.url().endsWith('/api/v1/links') && response.request().method() === 'POST');
    await dialog.getByRole('button', { name: 'Create link', exact: true }).click();
    const createdResponse = await created;
    assert.equal(createdResponse.status(), 201);
    const link = await createdResponse.json();
    await page.getByText('Test guide', { exact: true }).waitFor();

    await page.getByRole('button', { name: 'Create link', exact: true }).click();
    const unclean = target.replace('#start', '&utm_source=sample#start');
    const urlField = dialog.getByLabel('Destination URL', { exact: true });
    await urlField.fill(unclean);
    await dialog.getByRole('button', { name: 'Review URL parameters', exact: true }).click();
    await dialog.getByRole('button', { name: 'Use proposed URL', exact: true }).click();
    assert.equal(await urlField.inputValue(), target);
    await dialog.getByRole('button', { name: 'Find matching links', exact: true }).click();
    await dialog.getByRole('list', { name: 'Matching links', exact: true })
      .getByText('Test guide', { exact: true }).waitFor();
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();

    const redirect = await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 });
    assert.equal(redirect.status(), 302);
    assert.equal(redirect.headers().location, target);
    assert.equal((await page.request.get(publicOrigin + '/api/v1/auth/session')).status(), 404);

    await page.getByRole('button', { name: 'QR code', exact: true }).click();
    const qr = page.getByRole('dialog', { name: 'QR code', exact: true });
    await qr.getByRole('img', { name: 'QR code' }).waitFor();
    await qr.getByRole('combobox', { name: 'Resolution in pixels', exact: true }).selectOption('512');
    const downloaded = page.waitForEvent('download');
    await qr.getByRole('button', { name: 'Download PNG', exact: true }).click();
    const qrDownload = await downloaded;
    const png = await page.request.get(admin + '/api/v1/links/' + link.id + '/qr?format=png&size=512');
    assert.equal(qrDownload.suggestedFilename(), 'shortcircuit-link-' + link.id + '.png');
    assert.equal(png.status(), 200);
    assert.equal(png.headers()['content-type'], 'image/png');
    const pngBytes = await readFile(await qrDownload.path());
    assert.equal(pngBytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
    assert.equal(pngBytes.readUInt32BE(16), 512);
    await qr.getByRole('button', { name: 'Close', exact: true }).click();

    await page.getByRole('button', { name: 'Redirect settings', exact: true }).click();
    const policy = page.getByRole('dialog', { name: 'Redirect settings', exact: true });
    await policy.getByLabel('Maximum visits (optional)').fill('1');
    await policy.getByRole('button', { name: 'Save changes', exact: true }).click();
    await policy.waitFor({ state: 'hidden' });
    assert.equal((await page.request.head(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 302);
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 302);
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 404);

    await page.getByRole('button', { name: 'Analytics', exact: true }).click();
    await page.getByRole('heading', { name: 'Traffic summary', exact: true }).waitFor();
    await page.getByRole('checkbox', { name: 'Honor Do Not Track', exact: true }).waitFor();
    await page.getByRole('button', { name: 'Clear analytics data', exact: true }).click();
    const clear = page.getByRole('dialog', { name: 'Clear analytics data', exact: true });
    const cleared = page.waitForResponse(response =>
      response.url().endsWith('/api/v1/analytics/data') && response.request().method() === 'DELETE');
    await clear.getByRole('button', { name: 'Clear analytics data', exact: true }).click();
    assert.equal((await cleared).status(), 204);
    await page.getByText('Analytics data cleared.', { exact: true }).waitFor();

    await page.getByRole('button', { name: 'Data exchange', exact: true }).click();
    await page.getByLabel('Link data file', { exact: true }).setInputFiles({
      name: 'links.json',
      mimeType: 'application/json',
      buffer: Buffer.from(JSON.stringify({ version: 1, items: [{
        host: 'localhost',
        slug: 'backup-check',
        target: 'https://example.test/backup-check',
        title: 'Backup check',
        tags: [],
        enabled: true,
        analytics_enabled: true,
      }] })),
    });
    await page.getByRole('button', { name: 'Preview import', exact: true }).click();
    await page.getByText('Will create', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Apply import', exact: true }).click();
    await page.getByText('Import applied.', { exact: true }).waitFor();
    assert.equal((await page.request.get(publicOrigin + '/backup-check', { maxRedirects: 0 })).status(), 302);

    await page.getByRole('combobox', { name: 'Export format', exact: true }).selectOption('csv');
    const csvDownload = page.waitForEvent('download');
    await page.getByRole('button', { name: 'Download CSV page', exact: true }).click();
    const csvPage = await csvDownload;
    const csvText = await readFile(await csvPage.path(), 'utf8');
    assert.match(csvText, /^version,host,slug,target,title,tags,enabled,analytics_enabled/);

    await page.getByLabel('Link data file', { exact: true }).setInputFiles({
      name: 'links.csv',
      mimeType: 'text/csv',
      buffer: Buffer.from(
        'version,host,slug,target,title,tags,enabled,analytics_enabled\n' +
        '1,localhost,csv-check,https://example.test/csv-check,"Sample, title",[],true,true\n',
      ),
    });
    await page.getByRole('button', { name: 'Preview import', exact: true }).click();
    await page.getByText('localhost/csv-check', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Apply import', exact: true }).click();
    await page.getByText('Import applied.', { exact: true }).waitFor();
    const csvRedirect = await page.request.get(publicOrigin + '/csv-check', { maxRedirects: 0 });
    assert.equal(csvRedirect.headers().location, 'https://example.test/csv-check');

    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await page.getByRole('button', { name: 'Sign in', exact: true }).waitFor();
    assert.equal((await page.request.get(admin + '/api/v1/links')).status(), 401);
    assert.deepEqual(errors, []);
    console.log('Browser smoke passed: setup, create link, QR, visit limits, analytics controls, JSON and CSV exchange, URL tools, mobile layout and sign-out.');
  } finally {
    await browser.close();
  }
}

verify().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
