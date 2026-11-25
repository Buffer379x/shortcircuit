const { chromium } = require(process.env.SHORTCIRCUIT_PLAYWRIGHT || 'playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const { pathToFileURL }=require('node:url');
const path = require('node:path');
const base = path.resolve(__dirname, '..');
const file=pathToFileURL(base+'/prototype/index.html').href;
(async()=>{
 const browser=await chromium.launch();
 try {
  const p=await browser.newPage({viewport:{width:1440,height:900},acceptDownloads:true});
  const errors=[];
  const requests=[];
  p.on('pageerror', e=>errors.push(e.message));
  p.on('console', e=>{if(e.type()==='error') errors.push(e.text())});
  p.on('request', req=>{ if (/^https?:/.test(req.url())) requests.push(req.url()); });
  const titles={overview:'Overview',links:'Links','link/1':'Home lab handbook',analytics:'Analytics',qr:'QR Studio',
    health:'Health Monitor',domains:'Domains',settings:'Settings',login:'Sign in'};
  const go=async hash=>{
    await p.goto(file+'#'+hash);
    if (titles[hash]) await p.getByRole('heading',{level:1,name:titles[hash],exact:true}).waitFor();
  };
  const shots={
    overview:'dashboard', links:'links','link/1':'link-details', analytics:'analytics', qr:'qr-studio',health:'health-monitor',
    domains:'domains', settings:'settings',login:'login'
  };
  const dirs=base+'/assets/screenshots/';
  fs.mkdirSync(dirs,{recursive:true});
  for(const [view,name] of Object.entries(shots)) {
   await go(view);
   await p.screenshot({path:dirs+'desktop-'+name+'.png',animations:'disabled'});
  }
  await go('links');
  await p.getByRole('button',{name:'Create link',exact:true}).click();
  await p.screenshot({path:dirs+'desktop-create-link.png',animations:'disabled'});
  assert.equal(await p.getByRole('dialog').count(),1);
  await p.keyboard.press('Escape');
  assert.equal(await p.getByRole('dialog').count(),0);
  await p.getByRole('button',{name:'Toggle color theme'}).click();
  assert.equal(await p.locator('html').getAttribute('data-theme'),'light');
  await go('overview');
  await p.screenshot({path:dirs+'desktop-dashboard-light.png',animations:'disabled'});
  await p.getByRole('button',{name:'Toggle color theme'}).click();
  await p.setViewportSize({width:390,height:844});
  await p.screenshot({path:dirs+'mobile-dashboard.png',animations:'disabled'});
  await p.getByRole('button',{name:'Create link',exact:true}).click();
  await p.screenshot({path:dirs+'mobile-create-link.png',animations:'disabled'});
  await p.keyboard.press('Escape');
  const widths=[[1920,1080],[1440,900],[1280,800],[1024,768],[768,1024],[390,844],[320,568]];
  const checks=[];
  for(const [width,height] of widths) {
   await p.setViewportSize({width,height});
   for(const view of Object.keys(shots)) {
    await go(view);
    const overflow=await p.evaluate(()=>document.documentElement.scrollWidth>innerWidth);
    assert.equal(overflow,false,view+' page overflow at '+width);
   }
   await go('overview');
   await p.getByRole('button',{name:'Create link',exact:true}).click();
   assert.equal(await p.getByRole('dialog').evaluate(el=>el.scrollWidth>el.clientWidth),false,'dialog overflow at '+width);
   await p.keyboard.press('Escape');
   checks.push(width+'×'+height);
  }
  await p.setViewportSize({width:390,height:844});
  await go('overview');
  await p.getByRole('button',{name:'Toggle navigation'}).click();
  await p.getByRole('navigation',{name:'Main navigation'}).getByRole('link',{name:'Links',exact:true}).click();
  await p.waitForURL(url=>url.hash==='#links');
  await p.getByRole('searchbox',{name:'Search links'}).fill('wifi');
  assert.equal(await p.locator('tbody tr').count(),1);
  assert.match(await p.locator('tbody').innerText(),/Guest Wi-Fi/);
  await p.getByRole('searchbox',{name:'Search links'}).fill('unknown result');
  assert.match(await p.locator('.empty').innerText(),/No matching links/);
  await p.getByRole('button',{name:'Create link',exact:true}).click();
  await p.getByLabel('Destination URL').fill('https://destination.example/test?fixed=1#start');
  await p.getByLabel('Custom slug').fill('lab/handbook');
  await p.getByRole('dialog').getByRole('button',{name:'Create link',exact:true}).click();
  assert.match(await p.locator('#form-error').innerText(),/unique slug/);
  await p.getByLabel('Custom slug').fill('test/new-link');
  await p.getByRole('dialog').getByRole('button',{name:'Create link',exact:true}).click();
  await p.waitForURL(url=>url.hash==='#link/7');
  assert.match(await p.locator('dl').innerText(),/destination\.example\/test\?fixed=1#start/);
  await p.getByRole('button',{name:'Edit link'}).click();
  await p.getByLabel('Destination URL').fill('https://destination.example/updated');
  await p.getByRole('button',{name:'Save changes'}).click();
  assert.match(await p.locator('dl').innerText(),/destination\.example\/updated/);
  await p.reload();
  assert.match(await p.locator('dl').innerText(),/destination\.example\/updated/);
  await go('qr');
  await p.getByLabel('Error correction').selectOption('H');
  for(const kind of ['SVG','PNG']) {
   const result=p.waitForEvent('download');
   await p.getByRole('button',{name:'Download '+kind}).click();
   const image=await result;
   const downloaded=fs.readFileSync(await image.path());
   assert.ok(downloaded.length>100);
   assert.ok(downloaded.includes(kind==='SVG'?Buffer.from('<svg'):Buffer.from([137,80,78,71])));
  }
  await go('domains');
  await p.getByRole('button',{name:'Add domain'}).click();
  await p.getByLabel('Hostname').fill('trial.homelab.example');
  await p.getByRole('button',{name:'Save domain'}).click();
  assert.match(await p.locator('tbody').innerText(),/trial\.homelab\.example/);
  await go('settings');
  await p.getByLabel('Language').selectOption('de');
  await p.getByRole('button',{name:'Save changes'}).click();
  assert.equal(await p.getByRole('heading',{name:'Einstellungen'}).count(),1);
  await go('login');
  assert.equal(await p.getByRole('heading',{name:'Anmelden'}).count(),1);
  assert.equal(errors.length,0,JSON.stringify(errors));
  assert.equal(requests.length,0,'external requests');
  console.log('Chromium '+browser.version()+': 9 views + dialog across '+checks.join(', '));
  console.log('Captured 13 PNG files in assets/screenshots.');
  console.log('Verified navigation, creation, collision, editing and persistence, search and empty state, theme, language, QR downloads, domain demo, login and zero page errors/external HTTP requests.');
 } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exit(1)});
