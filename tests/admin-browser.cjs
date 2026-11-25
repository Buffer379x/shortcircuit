const assert = require('node:assert/strict');
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

    const redirect = await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 });
    assert.equal(redirect.status(), 302);
    assert.equal(redirect.headers().location, target);
    assert.equal((await page.request.get(publicOrigin + '/api/v1/auth/session')).status(), 404);

    await page.getByRole('button', { name: 'QR code', exact: true }).click();
    const qr = page.getByRole('dialog', { name: 'QR code', exact: true });
    await qr.getByRole('img', { name: 'QR code' }).waitFor();
    const png = await page.request.get(admin + '/api/v1/links/' + link.id + '/qr?format=png');
    assert.equal(png.status(), 200);
    assert.equal(png.headers()['content-type'], 'image/png');
    assert.equal((await png.body()).subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
    await qr.getByRole('button', { name: 'Close', exact: true }).click();

    await page.getByRole('button', { name: 'Redirect settings', exact: true }).click();
    const policy = page.getByRole('dialog', { name: 'Redirect settings', exact: true });
    await policy.getByLabel('Maximum visits (optional)').fill('1');
    await policy.getByRole('button', { name: 'Save changes', exact: true }).click();
    await policy.waitFor({ state: 'hidden' });
    assert.equal((await page.request.head(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 302);
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 302);
    assert.equal((await page.request.get(publicOrigin + '/test/guide', { maxRedirects: 0 })).status(), 404);

    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await page.getByRole('button', { name: 'Sign in', exact: true }).waitFor();
    assert.equal((await page.request.get(admin + '/api/v1/links')).status(), 401);
    assert.deepEqual(errors, []);
    console.log('Browser smoke passed: setup, create link, QR, visit limits, mobile layout and sign-out.');
  } finally {
    await browser.close();
  }
}

verify().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
