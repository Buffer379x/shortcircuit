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
    for (const [name, type] of [['favicon.svg', 'image/svg+xml'], ['favicon.png', 'image/png']]) {
      assert.equal(await page.locator(`link[rel="icon"][href="/${name}"]`).count(), 1);
      const favicon = await page.request.get(admin + '/' + name);
      assert.equal(favicon.status(), 200);
      assert.equal(favicon.headers()['content-type'], type);
      assert.deepEqual(await favicon.body(), await readFile('assets/branding/' + name));
      assert.equal((await page.request.get(publicOrigin + '/' + name)).status(), 404);
    }
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

    const specResponse = await page.request.get(admin + '/api/v1/openapi.json');
    assert.equal(specResponse.status(), 200);
    const spec = await specResponse.json();
    assert.equal(spec.openapi, '3.1.0');
    assert.ok(spec.paths['/api/v1/links/{id}/policy'].patch);
    assert.equal((await page.request.get(publicOrigin + '/api/v1/openapi.json')).status(), 404);

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
    await page.setViewportSize({ width: 390, height: 844 });
    await qr.getByLabel('Foreground color', { exact: true }).fill('#002040');
    await qr.getByLabel('Background color', { exact: true }).fill('#fff4e0');
    await qr.getByRole('checkbox', { name: 'Transparent background', exact: true }).check();
    await qr.getByRole('combobox', { name: 'Error correction', exact: true }).selectOption('H');
    await qr.getByRole('combobox', { name: 'Quiet zone in modules', exact: true }).selectOption('8');
    const qrFrame = qr.getByRole('combobox', { name: 'QR frame', exact: true });
    assert.equal(await qrFrame.inputValue(), 'none');
    await qrFrame.selectOption('outline');
    const styledPreview = qr.locator('img[src*="frame=outline"]');
    await styledPreview.waitFor({ state: 'visible' });
    const previewUrl = new URL(await styledPreview.getAttribute('src'), admin);
    assert.deepEqual(Object.fromEntries(previewUrl.searchParams), {
      format: 'svg', size: '512', foreground: '002040', background: 'fff4e0',
      transparent: 'true', correction: 'H', quiet_zone: '8', frame: 'outline',
    });
    assert.equal(await qr.evaluate(dialog => dialog.scrollWidth > dialog.clientWidth), false);
    const styledDownload = page.waitForEvent('download');
    await qr.getByRole('button', { name: 'Download SVG', exact: true }).click();
    const svgDownload = await styledDownload;
    const previewResponse = await page.request.get(previewUrl.href);
    assert.equal(previewResponse.status(), 200);
    const previewBytes = await previewResponse.body();
    assert.deepEqual(await readFile(await svgDownload.path()), previewBytes);
    assert.match(previewBytes.toString('utf8'), /fill="#002040"/);
    assert.doesNotMatch(previewBytes.toString('utf8'), /<rect\b/);
    const copyQr = qr.getByRole('button', { name: 'Copy PNG', exact: true });
    if (await copyQr.count()) {
      await page.context().grantPermissions(['clipboard-read', 'clipboard-write'], { origin: admin });
      await copyQr.click();
      await qr.getByText('QR code copied.', { exact: true }).waitFor();
      assert.equal(await page.evaluate(async () => (await navigator.clipboard.read())
        .some(item => item.types.includes('image/png'))), true);
    }
    await qr.getByText('QR templates', { exact: true }).click();
    await qr.getByRole('textbox', { name: 'Template name', exact: true }).fill('Print style');
    await qr.getByRole('button', { name: 'Save current settings', exact: true }).click();
    await qr.getByText('Template saved.', { exact: true }).waitFor();
    await qr.getByLabel('Foreground color', { exact: true }).fill('#eeeeee');
    await qr.getByText('Choose a darker foreground and a lighter background for enough contrast.', { exact: true }).waitFor();
    assert.equal(await qr.getByRole('button', { name: 'Download PNG', exact: true }).isDisabled(), true);
    await qr.getByRole('button', { name: 'Close', exact: true }).click();
    await page.getByRole('button', { name: 'QR code', exact: true }).click();
    await qr.getByText('QR templates', { exact: true }).click();
    const template = qr.getByRole('combobox', { name: 'Saved template', exact: true });
    await template.selectOption({ label: 'Print style' });
    await qr.getByRole('button', { name: 'Apply template', exact: true }).click();
    await styledPreview.waitFor({ state: 'visible' });
    assert.equal(await qrFrame.inputValue(), 'outline');
    assert.equal(await styledPreview.getAttribute('src'), previewUrl.pathname + previewUrl.search);
    assert.equal(await qr.evaluate(dialog => dialog.scrollWidth > dialog.clientWidth), false);
    const removeTemplate = qr.getByRole('button', { name: 'Delete template', exact: true });
    const confirmTemplate = page.getByRole('dialog', { name: 'Delete this QR template?', exact: true });
    await removeTemplate.click();
    await confirmTemplate.getByRole('button', { name: 'Cancel', exact: true }).click();
    await confirmTemplate.waitFor({ state: 'hidden' });
    assert.equal(await template.getByRole('option', { name: 'Print style' }).count(), 1);
    await removeTemplate.click();
    await confirmTemplate.getByRole('button', { name: 'Delete template', exact: true }).click();
    await confirmTemplate.waitFor({ state: 'hidden' });
    await qr.getByText('Template deleted.', { exact: true }).waitFor();
    assert.equal(await template.getByRole('option', { name: 'Print style' }).count(), 0);
    assert.equal(await styledPreview.getAttribute('src'), previewUrl.pathname + previewUrl.search);
    await qr.getByRole('combobox', { name: 'Error correction', exact: true }).selectOption('Q');
    const logo = qr.getByLabel('PNG logo', { exact: true });
    await logo.setInputFiles({
      name: 'fixture.png',
      mimeType: 'image/png',
      buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAABAAAAAICAYAAADwdn+XAAAAFUlEQVR4nGNQTFv8nxLMMGrAcDAAAC6wFJBQyGgvAAAAAElFTkSuQmCC', 'base64'),
    });
    const logoPreview = qr.locator('canvas[role="img"]:visible');
    await logoPreview.waitFor();
    assert.equal(await qr.getByRole('combobox', { name: 'Error correction', exact: true }).inputValue(), 'H');
    assert.deepEqual(await logoPreview.evaluate(canvas =>
      [...canvas.getContext('2d').getImageData(canvas.width / 2, canvas.height / 2, 1, 1).data]), [33, 102, 163, 255]);
    const logoDownload = page.waitForEvent('download');
    await qr.getByRole('button', { name: 'Download SVG', exact: true }).click();
    assert.match(await readFile(await (await logoDownload).path(), 'utf8'), /fill="#2166a3"/);
    await qr.getByRole('button', { name: 'Remove logo', exact: true }).click();
    await logoPreview.waitFor({ state: 'hidden' });
    await styledPreview.waitFor({ state: 'visible' });
    await qr.getByRole('button', { name: 'Close', exact: true }).click();
    await page.setViewportSize({ width: 1360, height: 900 });

    const guideRow = page.getByRole('row').filter({ hasText: 'localhost/test/guide' });
    await guideRow.getByRole('button', { name: 'Edit link', exact: true }).click();
    const editGuide = page.getByRole('dialog', { name: 'Edit link', exact: true });
    await editGuide.getByLabel('Destination URL', { exact: true }).fill('https://example.test/new-guide');
    await editGuide.getByLabel('Title (optional)', { exact: true }).fill('Updated guide');
    await editGuide.getByRole('button', { name: 'Save changes', exact: true }).click();
    await editGuide.waitFor({ state: 'hidden' });
    await page.setViewportSize({ width: 390, height: 844 });
    await guideRow.getByRole('button', { name: 'Link history', exact: true }).click();
    const history = page.getByRole('dialog', { name: 'Link history', exact: true });
    await history.getByRole('button', { name: 'Compare', exact: true }).click();
    await history.getByRole('heading', { name: 'Compare with current settings', exact: true }).waitFor();
    assert.equal(await history.evaluate(dialog => dialog.scrollWidth > dialog.clientWidth), false);
    await history.getByText(target, { exact: true }).waitFor();
    await history.getByText('https://example.test/new-guide', { exact: true }).waitFor();
    const exportHistory = page.waitForEvent('download');
    await history.getByRole('button', { name: 'Export history', exact: true }).click();
    const historyFile = await exportHistory;
    const historyJson = JSON.parse(await readFile(await historyFile.path(), 'utf8'));
    assert.equal(historyJson.items[0].previous.value.target, target);
    await history.getByRole('button', { name: 'Restore', exact: true }).last().click();
    const restore = page.getByRole('dialog', { name: 'Restore this revision?', exact: true });
    await restore.getByRole('button', { name: 'Restore', exact: true }).click();
    await restore.waitFor({ state: 'hidden' });
    await history.getByText('Revision restored.', { exact: true }).waitFor();
    await history.getByRole('button', { name: 'Clear history', exact: true }).click();
    const clearHistory = page.getByRole('dialog', { name: 'Clear link history?', exact: true });
    await clearHistory.getByRole('button', { name: 'Clear history', exact: true }).click();
    await history.getByText('No changes recorded yet.', { exact: true }).waitFor();
    await history.getByRole('button', { name: 'Close', exact: true }).click();
    await guideRow.getByText('Test guide', { exact: true }).waitFor();
    await page.setViewportSize({ width: 1360, height: 900 });
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).headers().location, target);

    await page.getByRole('button', { name: 'Redirect settings', exact: true }).click();
    const policy = page.getByRole('dialog', { name: 'Redirect settings', exact: true });
    await policy.getByLabel('Maximum visits (optional)').fill('1');
    await policy.getByRole('button', { name: 'Save changes', exact: true }).click();
    await policy.waitFor({ state: 'hidden' });
    assert.equal((await page.request.head(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 302);
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 302);
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 404);

    const protectedTarget = admin + '/readyz';
    const linkPassword = 'ci-browser-link-password';
    await page.getByRole('button', { name: 'Create link', exact: true }).click();
    await dialog.getByLabel('Destination URL', { exact: true }).fill(protectedTarget);
    await dialog.getByLabel('Slug (optional)', { exact: true }).fill('protected-test');
    await dialog.getByText('Link protection', { exact: true }).click();
    await dialog.getByLabel('Link password (optional)', { exact: true }).fill(linkPassword);
    const protectedCreated = page.waitForResponse(response =>
      response.url().endsWith('/api/v1/links') && response.request().method() === 'POST');
    await dialog.getByRole('button', { name: 'Create link', exact: true }).click();
    const protectedResponse = await protectedCreated;
    assert.equal(protectedResponse.status(), 201);
    const protectedLink = await protectedResponse.json();
    assert.ok(!JSON.stringify(protectedLink).includes(linkPassword));
    await dialog.waitFor({ state: 'hidden' });

    const protectedRow = page.getByRole('row').filter({ hasText: 'localhost/protected-test' });
    await protectedRow.getByRole('button', { name: 'Link protection', exact: true }).click();
    const protection = page.getByRole('dialog', { name: 'Link protection', exact: true });
    await protection.getByText('Password protection is enabled.', { exact: true }).waitFor();
    assert.equal(await protection.getByLabel('New password', { exact: true }).inputValue(), '');
    await protection.getByRole('checkbox', { name: 'One-time link', exact: true }).check();
    const protectionSaved = page.waitForResponse(response =>
      response.url().endsWith(`/api/v1/links/${protectedLink.id}/protection`) && response.request().method() === 'PATCH');
    await protection.getByRole('button', { name: 'Save changes', exact: true }).click();
    const protectionResponse = await protectionSaved;
    assert.equal(protectionResponse.status(), 200);
    assert.deepEqual(protectionResponse.request().postDataJSON(), { one_time: true });
    assert.deepEqual(await protectionResponse.json(), { password_required: true, one_time: true, consumed: false });
    await protection.waitFor({ state: 'hidden' });

    const visitor = await browser.newPage();
    try {
      const shortUrl = publicOrigin + '/protected-test';
      assert.equal((await visitor.goto(shortUrl)).status(), 200);
      await visitor.getByRole('heading', { name: 'Confirm link access', exact: true }).waitFor();
      await visitor.getByLabel('Password', { exact: true }).fill(linkPassword);
      const [destination] = await Promise.all([
        visitor.waitForRequest(request => request.url() === protectedTarget),
        visitor.waitForURL(protectedTarget),
        visitor.getByRole('button', { name: 'Continue', exact: true }).click(),
      ]);
      assert.equal(destination.method(), 'GET');
      assert.equal(destination.postData(), null);
      assert.equal((await visitor.goto(shortUrl)).status(), 404);
    } finally {
      await visitor.close();
    }

    await protectedRow.getByRole('button', { name: 'Link protection', exact: true }).click();
    await protection.getByText('This one-time link has already been used.', { exact: true }).waitFor();
    assert.equal(await protection.getByRole('button', { name: 'Save changes', exact: true }).count(), 0);
    await protection.getByRole('button', { name: 'Close', exact: true }).click();
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await page.setViewportSize({ width: 1360, height: 900 });

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
    assert.equal((await page.request.get(admin + '/api/v1/openapi.json')).status(), 401);
    assert.deepEqual(errors, []);
    console.log('Browser smoke passed: setup, create link, styled QR preview, templates, download and validation, link history, visit limits, protected links, analytics controls, JSON and CSV exchange, URL tools, mobile layout and sign-out.');
  } finally {
    await browser.close();
  }
}

verify().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
