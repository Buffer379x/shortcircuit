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
    await page.evaluate(() => { window.delayNextLogoPreview = true; });
    await logo.setInputFiles({
      name: 'fixture.png',
      mimeType: 'image/png',
      buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAABAAAAAICAYAAADwdn+XAAAAFUlEQVR4nGNQTFv8nxLMMGrAcDAAAC6wFJBQyGgvAAAAAElFTkSuQmCC', 'base64'),
    });
    await page.waitForFunction(() => window.delayedLogoPreview);
    await qr.getByRole('combobox', { name: 'Resolution in pixels', exact: true }).selectOption('768');
    await page.waitForFunction(() => window.abortedLogoPreview);
    const logoPreview = qr.locator('[role="img"] canvas:visible');
    await logoPreview.waitFor();
    assert.equal(await logoPreview.evaluate(canvas => canvas.width), 768);
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
    await policy.getByRole('button', { name: 'Add rule', exact: true }).click();
    const firstRule = policy.getByRole('group', { name: 'Rule 1', exact: true });
    await firstRule.getByLabel('Destination URL', { exact: true }).fill('https://example.test/simulator');
    await firstRule.getByLabel('Parameter name', { exact: true }).fill('team');
    await firstRule.getByLabel('Match value', { exact: true }).fill('ops');
    await policy.locator('summary').filter({ hasText: /^Redirect simulator$/ }).click();
    await policy.getByLabel('Query string', { exact: true }).fill('team=ops');
    await policy.getByRole('button', { name: 'Simulate redirect', exact: true }).click();
    await policy.getByText('https://example.test/simulator', { exact: true }).waitFor();
    await policy.getByText('302 · Rule 1', { exact: true }).waitFor();
    await policy.getByLabel('Query string', { exact: true }).fill('team=other');
    await policy.getByText('https://example.test/simulator', { exact: true }).waitFor({ state: 'hidden' });
    await firstRule.getByRole('button', { name: 'Remove rule', exact: true }).click();
    await policy.getByRole('checkbox', { name: 'Request exclusion from search', exact: false }).check();
    await policy.getByLabel('Maximum visits (optional)').fill('1');
    await policy.getByRole('button', { name: 'Save changes', exact: true }).click();
    await policy.waitFor({ state: 'hidden' });
    const headGuide = await page.request.head(publicOrigin + '/test/guide', { maxRedirects: 0 });
    assert.equal(headGuide.status(), 302);
    assert.equal(headGuide.headers()['x-robots-tag'], 'noindex, nofollow');
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
    await protectedRow.getByRole('button', { name: 'Duplicate link', exact: true }).click();
    const duplicate = page.getByRole('dialog', { name: 'Duplicate link', exact: true });
    assert.equal(await duplicate.getByLabel('Destination URL', { exact: true }).inputValue(), protectedTarget);
    assert.equal(await duplicate.getByLabel('Slug (optional)', { exact: true }).count(), 0);
    await duplicate.getByLabel('Description (optional)', { exact: true }).fill('Reviewed test description');
    const duplicateCreated = page.waitForResponse(response =>
      response.url().endsWith('/api/v1/links') && response.request().method() === 'POST');
    await duplicate.getByRole('button', { name: 'Create duplicate', exact: true }).click();
    const duplicateResponse = await duplicateCreated;
    assert.equal(duplicateResponse.status(), 201);
    assert.equal(duplicateResponse.request().postDataJSON().protection, undefined);
    assert.equal(duplicateResponse.request().postDataJSON().slug, undefined);
    const copied = await duplicateResponse.json();
    assert.equal(copied.enabled, false);
    assert.match(copied.slug, /^[a-zA-Z0-9_-]{10}$/);
    await duplicate.waitFor({ state: 'hidden' });
    assert.equal((await page.request.get(publicOrigin + '/' + copied.slug, { maxRedirects: 0 })).status(), 404);
    await page.getByRole('row').filter({ hasText: 'localhost/' + copied.slug })
      .getByRole('button', { name: 'Delete link', exact: true }).click();
    const deleteCopy = page.getByRole('dialog', { name: 'Delete link', exact: true });
    await deleteCopy.getByRole('button', { name: 'Delete', exact: true }).click();
    await deleteCopy.waitFor({ state: 'hidden' });

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
        description: 'Fixture from native exchange',
        tags: ['internal'],
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

    await page.getByRole('button', { name: 'Links', exact: true }).click();
    const linkTable = page.getByRole('table', { name: 'Links', exact: true });
    await linkTable.getByRole('columnheader', { name: 'Clicks', exact: true }).waitFor();
    await linkTable.getByRole('columnheader', { name: 'Last used', exact: true }).waitFor();
    await page.setViewportSize({ width: 768, height: 1024 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByRole('combobox', { name: 'Sort links', exact: true }).selectOption('slug_asc');
    await linkTable.locator('.link-short').first().filter({ hasText: 'localhost/backup-check' }).waitFor();
    assert.equal(await linkTable.locator('.link-short').first().textContent(), 'localhost/backup-check');
    const tagFilter = page.getByRole('combobox', { name: 'Filter by tags', exact: true });
    await tagFilter.selectOption('docs');
    await tagFilter.selectOption('internal');
    const backupRow = linkTable.getByRole('row').filter({ hasText: 'localhost/backup-check' });
    const guideLink = linkTable.getByRole('link', { name: 'localhost/test/guide', exact: true });
    await guideLink.waitFor();
    assert.equal(await backupRow.count(), 0);
    await page.getByRole('combobox', { name: 'Match tags', exact: true }).selectOption('any');
    await backupRow.waitFor();
    await backupRow.getByText('Fixture from native exchange', { exact: true }).waitFor();
    await guideLink.waitFor();
    await page.getByRole('button', { name: 'Remove tag filter docs', exact: true }).click();
    await page.getByRole('button', { name: 'Remove tag filter internal', exact: true }).click();

    const view = page.getByRole('combobox', { name: 'Link view', exact: true });
    const favorite = backupRow.getByRole('button', { name: 'Favorite', exact: true });
    await favorite.click();
    await page.getByText('Link added to favorites.', { exact: true }).waitFor();
    await view.selectOption('favorites');
    await backupRow.waitFor();
    assert.equal(await favorite.getAttribute('aria-pressed'), 'true');
    const organizationPath = /\/api\/v1\/links\/\d+\/organization$/;
    let releaseArchive;
    const archivePending = new Promise(resolve => { releaseArchive = resolve; });
    await page.route(organizationPath, async route => {
      await archivePending;
      await route.continue();
    }, { times: 1 });
    const archiveSent = page.waitForRequest(organizationPath);
    await backupRow.getByRole('button', { name: 'Archive link', exact: true }).click();
    await archiveSent;
    const searchBox = page.getByRole('searchbox', { name: 'Search all links', exact: true });
    try {
      await searchBox.focus();
    } finally {
      releaseArchive();
    }
    await page.getByText('There are no favorites.', { exact: true }).waitFor();
    assert.equal(await searchBox.evaluate(input => input === document.activeElement), true);
    const archivedRedirect = await page.request.get(publicOrigin + '/backup-check', { maxRedirects: 0 });
    assert.equal(archivedRedirect.status(), 302);
    assert.equal(archivedRedirect.headers().location, 'https://example.test/backup-check');
    await view.selectOption('archived');
    await backupRow.getByRole('button', { name: 'Unarchive link', exact: true }).click();
    await page.getByText('There are no archived links.', { exact: true }).waitFor();
    await view.selectOption('favorites');
    await backupRow.waitFor();
    assert.equal(await favorite.getAttribute('aria-pressed'), 'true');
    await view.selectOption('unarchived');
    await page.getByRole('combobox', { name: 'Filter by domain', exact: true }).selectOption('localhost');
    const searchResponse = page.waitForResponse(response => response.url() === admin + '/api/v1/links/search'
      && response.request().postDataJSON()?.query === 'chapter=intro#start');
    await page.getByRole('searchbox', { name: 'Search all links', exact: true }).fill('chapter=intro#start');
    const found = await searchResponse;
    assert.equal(found.status(), 200);
    assert.equal(found.request().method(), 'POST');
    assert.deepEqual(found.request().postDataJSON(), { query: 'chapter=intro#start', host: 'localhost', view: 'unarchived', sort: 'slug_asc' });
    await linkTable.getByRole('link', { name: 'localhost/test/guide', exact: true }).waitFor();
    assert.equal(await linkTable.locator('tbody tr').count(), 1);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    const searchFailure = page.getByRole('alert').filter({ hasText: 'The service is unavailable' });
    const failNextSearch = () => page.route(admin + '/api/v1/links/search',
      route => route.fulfill({ status: 503, json: { code: 'service_unavailable' } }), { times: 1 });
    await failNextSearch();
    await linkTable.getByRole('button', { name: 'Disable', exact: true }).click();
    await searchFailure.waitFor();
    await linkTable.getByRole('button', { name: 'Enable', exact: true }).waitFor();
    const recoveredSearch = page.waitForResponse(response =>
      response.url() === admin + '/api/v1/links/search' && response.status() === 200);
    await page.getByRole('button', { name: 'Try again', exact: true }).click();
    await recoveredSearch;
    await searchFailure.waitFor({ state: 'hidden' });
    await linkTable.getByRole('button', { name: 'Enable', exact: true }).waitFor();

    await failNextSearch();
    await linkTable.getByRole('button', { name: 'Delete link', exact: true }).click();
    await page.getByRole('dialog', { name: 'Delete link', exact: true })
      .getByRole('button', { name: 'Delete', exact: true }).click();
    await searchFailure.waitFor();
    assert.equal(await linkTable.count(), 0);
    assert.equal(await page.getByRole('button', { name: 'Export shown links (JSON)', exact: true }).isDisabled(), true);
    await page.getByRole('searchbox', { name: 'Search all links', exact: true }).fill('');
    const selectProtected = linkTable.getByRole('checkbox', { name: 'Select link localhost/protected-test', exact: true });
    const selectCsv = linkTable.getByRole('checkbox', { name: 'Select link localhost/csv-check', exact: true });
    await selectProtected.check();
    await selectCsv.check();
    const bulk = page.getByRole('combobox', { name: 'Bulk actions', exact: true });
    const applyBulk = page.getByRole('button', { name: 'Apply to selection', exact: true });
    await bulk.selectOption('add_tags');
    const batchTags = page.getByRole('textbox', { name: 'Tags, separated by commas', exact: true });
    const longTagList = Array.from({ length: 20 }, (_, index) => 'ä'.repeat(20) + '🟩'.repeat(18) + String(index).padStart(2, '0')).join(', ');
    await batchTags.fill(longTagList);
    assert.equal(await batchTags.inputValue(), longTagList);
    await batchTags.fill('batch-review');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await applyBulk.click();
    await page.getByText('Selected links updated.', { exact: true }).waitFor();
    await linkTable.getByRole('button', { name: 'batch-review', exact: true }).first().waitFor();
    assert.equal(await linkTable.getByRole('button', { name: 'batch-review', exact: true }).count(), 2);
    await selectProtected.check();
    await selectCsv.check();
    await bulk.selectOption('export_json');
    const batchDownload = page.waitForEvent('download');
    await applyBulk.click();
    const selectedFile = JSON.parse(await readFile(await (await batchDownload).path(), 'utf8'));
    assert.equal(selectedFile.version, 1);
    assert.deepEqual(selectedFile.items.map(item => [item.slug, item.enabled]), [['protected-test', false], ['csv-check', true]]);
    await bulk.selectOption('delete');
    await applyBulk.click();
    const confirmBulk = page.getByRole('dialog', { name: 'Delete selected links?', exact: true });
    await confirmBulk.getByText('localhost/csv-check', { exact: true }).waitFor();
    await confirmBulk.getByRole('button', { name: 'Cancel', exact: true }).click();
    await confirmBulk.waitFor({ state: 'hidden' });
    assert.equal(await selectCsv.isChecked(), true);
    await applyBulk.click();
    await confirmBulk.getByRole('button', { name: 'Delete selected', exact: true }).click();
    await page.getByText('Selected links deleted.', { exact: true }).waitFor();
    await selectCsv.waitFor({ state: 'hidden' });
    assert.equal(await selectProtected.count(), 0);
    await linkTable.getByRole('link', { name: 'localhost/backup-check', exact: true }).waitFor();

    await page.getByRole('button', { name: 'Tags', exact: true }).click();
    const tagRow = page.locator('.catalog-row').filter({ has: page.getByText('internal', { exact: true }) });
    const editTagDetails = tagRow.getByRole('button', { name: 'Edit tag details', exact: true });
    await editTagDetails.focus();
    await editTagDetails.press('Enter');
    const tagDetails = page.getByRole('dialog', { name: 'Edit tag details', exact: true });
    await tagDetails.getByLabel('Use a custom color', { exact: true }).check();
    await tagDetails.getByLabel('Tag color', { exact: true }).fill('#2d7aed');
    await tagDetails.getByLabel('Tag description', { exact: true }).fill('Team links');
    await tagDetails.getByRole('button', { name: 'Save changes', exact: true }).click();
    await tagDetails.waitFor({ state: 'hidden' });
    await tagRow.getByText('Team links', { exact: true }).waitFor();
    assert.equal(await editTagDetails.evaluate(button => button === document.activeElement), true);

    await page.getByRole('button', { name: 'Links', exact: true }).click();
    const internalTag = linkTable.getByRole('button', { name: 'internal', exact: true });
    await internalTag.locator('span').waitFor();
    assert.equal(await internalTag.getAttribute('title'), 'Team links');
    assert.equal(await internalTag.locator('span').evaluate(mark => mark.style.getPropertyValue('--tag-color')), '#2d7aed');
    await page.getByRole('button', { name: 'Create link', exact: true }).click();
    const tagField = dialog.getByLabel('Tags', { exact: true });
    await tagField.fill('other, inte');
    await dialog.getByRole('group', { name: 'Suggested tags', exact: true })
      .getByRole('button', { name: 'internal', exact: true }).click();
    await page.waitForFunction(() => document.activeElement?.getAttribute('name') === 'tags'
      && document.activeElement.value === 'other, internal, ');
    assert.equal((await tagField.inputValue()).trimEnd(), 'other, internal,');
    assert.equal(await tagField.evaluate(input => input === document.activeElement), true);
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);

    await page.getByRole('button', { name: 'Domains', exact: true }).click();
    const localDomain = page.locator('.catalog-row').filter({ has: page.getByText('localhost', { exact: true }) });
    await localDomain.getByRole('button', { name: 'Edit domain', exact: true }).click();
    const domainSettings = page.getByRole('dialog', { name: 'Edit domain', exact: true });
    await domainSettings.getByRole('checkbox', { name: 'Discourage crawling', exact: false }).check();
    await domainSettings.getByRole('button', { name: 'Save changes', exact: true }).click();
    await domainSettings.waitFor({ state: 'hidden' });
    const robots = await page.request.get(publicOrigin + '/robots.txt');
    assert.equal(robots.status(), 200);
    assert.equal(await robots.text(), 'User-agent: *\nDisallow: /\n');

    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await page.getByRole('button', { name: 'Sign in', exact: true }).waitFor();
    assert.equal((await page.request.get(admin + '/api/v1/links')).status(), 401);
    assert.equal((await page.request.get(admin + '/api/v1/openapi.json')).status(), 401);
    assert.deepEqual(errors, []);
    console.log('Browser smoke passed: setup, create link, styled QR preview, templates, download and validation, link history, visit limits, protected links, analytics controls, JSON and CSV exchange, URL tools, catalog search, bulk actions, mobile layout and sign-out.');
  } finally {
    await browser.close();
  }
}

verify().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
