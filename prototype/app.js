"use strict";
const paths = {
  grid: 'M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z',
  link: 'M10 13a4 4 0 0 0 6 .4l4-4a4 4 0 0 0-5.7-5.7l-2.3 2.4 M14 11a4 4 0 0 0-6-.4l-4 4a4 4 0 0 0 5.7 5.7l2.3-2.4',
  chart: 'M4 4v16h16 M8 15l4-5 4 2 4-6',
  qr: 'M3 3h6v6H3z M15 3h6v6h-6z M3 15h6v6H3z M15 15h3v3h-3z M21 15v3 M15 21h3 M21 21h.01 M12 3v4m-9 5h4',
  activity: 'M3 12h4l3-7 4 14 3-7h4',
  globe: 'M22 12a10 10 0 1 1-20 0 10 10 0 1 1 20 0z M2 12h20 M12 2a15 15 0 0 1 0 20m0-20a15 15 0 0 0 0 20',
  settings: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z M10 3h4l.6 2.1 2 .9 2-.9 2.8 2.8-.9 2 .9 2 2.1.6v4l-2.1.6-.9 2 .9 2-2.8 2.8-2-.9-2 .9-.6 2.1h-4l-.6-2.1-2-.9-2 .9-2.8-2.8 .9-2-.9-2L.5 16v-4l2.1-.6 .9-2-.9-2 2.8-2.8 2 .9 2-.9z',
  plus: 'M12 5v14 M5 12h14', chevron: 'm9 6 6 6-6 6', down: 'm6 9 6 6 6-6', arrow: 'M4 12h15m-6-6 6 6-6 6',
  diagonal: 'M7 17 17 7 M7 7h10v10', close: 'm6 6 12 12M18 6 6 18',
  sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 1 0 0-8z M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5',
  moon: 'M20.5 15A9 9 0 0 1 9 3.5 9 9 0 1 0 20.5 15z',
  calendar: 'M4 5h16v16H4z M8 3v4m8-4v4M4 9h16',
  copy: 'M8 8h12v12H8z M16 4H4v12',
  search: 'M17 17l4 4 M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0z',
  menu: 'M4 6h16M4 12h16M4 18h16',
  cursor: 'm5 3 14 10-7 1-3 7z',
  warning: 'M12 3 2 20h20z M12 9v4m0 3h.01',
  check: 'm5 12 4 4L19 6', filter: 'M3 6h18M6 12h12m-8 6h4',
  download: 'M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5', layers: 'm12 3 9 5-9 5-9-5z M3 12l9 5 9-5M3 16l9 5 9-5',
  shield: 'M12 3 4 6v6c0 4 3 7 8 9 5-2 8-5 8-9V6z M9 12l2 2 4-4',
  adjust: 'M4 7h16M4 17h16M8 4v6m8 4v6', refresh: 'M20 5v5h-5 M20 10a8 8 0 1 0 0 5',
  edit: 'm14 6 4 4M5 19l4-.6L20 7l-4-4L5 14z', clock: 'M22 12a10 10 0 1 1-20 0 10 10 0 1 1 20 0z M12 6v6l4 2'
};
const icon = name => `<svg class="icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="${paths[name]}"/></svg>`;
const esc = value => String(value ?? "").replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
const state = {
  lang: localStorage.getItem('sc-lang') || 'en',
  theme: localStorage.getItem('sc-theme') || 'dark',
  range: '30', search: '', domain: '', status: '', sort: 'recent',
  filterHealth: '', detailTab: 'details', setting: 'general', qr: 1,
  strength: 'M', quiet: 4, foreground: '#192320', background: '#fdfefb', transparent: false,
  menu: false, selected: new Set(), lastHealth: {}, settings: {},
};
const t = (english, german) => state.lang === 'de' ? german : english;
const n = value => new Intl.NumberFormat(t('en-US','de-DE')).format(value);
const safeStored = (name, fallback) => {
  try { return JSON.parse(localStorage.getItem(name)) ?? fallback; } catch { return fallback; }
};
const starterLinks = [
  {id:1, name:'Home lab handbook', slug:'lab/handbook', domain:'go.homelab.example', target:'https://docs.homelab.example/handbook?ref=internal', tags:['homelab','docs'], visits:1842, status:'Active', health:'Healthy', day:'Oct 28', deDay:'28. Okt.', symbol:'layers'},
  {id:2, name:'Guest Wi-Fi', slug:'wifi', domain:'go.homelab.example', target:'https://portal.homelab.example/guest', tags:['homelab'], visits:927, status:'Active', health:'Healthy', day:'Oct 26', deDay:'26. Okt.', symbol:'link'},
  {id:3, name:'Workshop registration', slug:'workshop', domain:'s.homelab.example', target:'https://events.example.org/2026/workshop', tags:['events'], visits:406, status:'Active', expires:'2026-11-04', health:'Warning', day:'Oct 23', deDay:'23. Okt.', symbol:'calendar'},
  {id:4, name:'Release notes', slug:'releases', domain:'go.homelab.example', target:'https://docs.homelab.example/releases', tags:['docs'], visits:673, status:'Active', health:'Redirected', day:'Oct 21', deDay:'21. Okt.', symbol:'layers'},
  {id:5, name:'Photo archive', slug:'photos/2024', domain:'go.homelab.example', target:'https://files.homelab.example/archive/2024', tags:['archive'], visits:256, status:'Active', health:'Broken', day:'Oct 19', deDay:'19. Okt.', symbol:'link'},
  {id:6, name:'Project status', slug:'status', domain:'s.homelab.example', target:'https://status.homelab.example', tags:['homelab'], visits:518, status:'Archived', health:'Not checked', day:'Oct 14', deDay:'14. Okt.', symbol:'activity'},
];
let links = safeStored('sc-links', starterLinks);
let domains = safeStored('sc-domains', [
  {name:'go.homelab.example', primary:true, status:'Healthy', fallback:'https://homelab.example/links', count:158},
  {name:'s.homelab.example', primary:false, status:'Healthy', fallback:'', count:90},
]);
const screen = document.querySelector('#app');
const dialog = document.querySelector('#dialog');
const notice = document.querySelector('#feedback');
const shortcut = link => `https://${link.domain}/${link.slug}`;
const demoDate = Date.UTC(2026,9,31);
const daysToExpiry = link => Math.round((Date.parse(link.expires)-demoDate)/86_400_000);
function linkStatus(link) {
  if (['Archived','Disabled'].includes(link.status) || !link.expires) return link.status;
  const days = daysToExpiry(link);
  return days < 0 ? 'Expired' : days < 7 ? 'Expiring' : 'Active';
}
const views = [
  ['overview','grid','Overview','Übersicht'], ['links','link','Links','Links'],
  ['analytics','chart','Analytics','Statistiken'], ['qr','qr','QR Studio','QR-Studio'],
  ['health','activity','Health Monitor','Linkprüfung'], ['domains','globe','Domains','Domains'],
  ['settings','settings','Settings','Einstellungen']
];
const names = {
  Active:['Active','Aktiv'], Expiring:['Expiring','Läuft ab'], Archived:['Archived','Archiviert'],
  Disabled:['Disabled','Deaktiviert'], Expired:['Expired','Abgelaufen'], Healthy:['Healthy','Erreichbar'], Warning:['Warning','Warnung'],
  Broken:['Broken','Defekt'], Redirected:['Redirected','Umgeleitet'], 'Not checked':['Not checked','Ungeprüft'],
};
const label = value => names[value] ? t(...names[value]) : value;
const tones = {Active:'success', Expiring:'warn', Expired:'neutral', Archived:'neutral', Disabled:'neutral', Healthy:'success', Warning:'warn', Broken:'bad', Redirected:'info', 'Not checked':'neutral'};
const badge = value => `<span class="status ${tones[value] || 'neutral'}"><span class="dot"></span>${esc(label(value))}</span>`;
function button(action, caption, {kind='', glyph='', value='', title=caption}={}) {
  return `<button type="button" class="btn ${kind}" data-action="${action}" data-value="${esc(value)}" title="${esc(title)}">${glyph ? icon(glyph) : ''}<span>${esc(caption)}</span></button>`;
}
function iconButton(action, glyph, name, value='', className='') {
  return `<button type="button" class="icon-button ghost ${className}" aria-label="${esc(name)}" title="${esc(name)}" data-action="${action}" data-value="${esc(value)}">${icon(glyph)}</button>`;
}
const pageTitle = (title, help, actions='') => `<div class="page-head"><div><h1>${title}</h1>${help ? '<p>'+help+'</p>' : ''}</div>${actions}</div>`;
const primary = () => button('create',t('Create link','Link erstellen'),{kind:'primary', glyph:'plus'});
const brand = className => `<a href="#overview" class="brand ${className}"><img src="../assets/branding/app-icon.svg" alt="">ShortCircuit</a>`;
function route() {
  const [page,rawId] = location.hash.slice(1).split('/');
  return {page: views.some(view => view[0]===page) || ['link','login'].includes(page) ? page : 'overview', id:Number(rawId)};
}
function syncTheme() {
  const resolved = state.theme === 'system' ? (matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark') : state.theme;
  document.documentElement.dataset.theme = resolved;
  document.documentElement.lang = state.lang;
}
function shell(page, content) {
  const current = page === 'link' ? 'links' : page;
  const title = t(...views.find(view => view[0] === current).slice(2));
  const theme = document.documentElement.dataset.theme;
  return `<button class="nav-shade" ${state.menu ? '' : 'hidden'} data-action="menu" aria-label="${t('Close navigation','Navigation schließen')}"></button>
    <aside class="rail ${state.menu ? 'is-open' : ''}" id="side-nav">${brand('')}
      <nav aria-label="${t('Main navigation','Hauptnavigation')}">${views.map(([id,glyph,en,de]) =>
        (id === 'settings' ? '<hr>' : '') + `<a class="nav-item" href="#${id}" ${id === current ? 'aria-current="page"' : ''}>${icon(glyph)}${t(en,de)}</a>`
      ).join('')}</nav>
      <div class="rail-foot"><button type="button" class="profile row" data-action="profile"><span class="avatar">AD</span><span class="profile-text"><strong class="profile-name">Alex Devlin</strong><small>${t('Administrator','Administrator')}</small></span>${icon('chevron')}</button></div>
    </aside>
    <div class="workspace">
      <header class="topbar">
        <div class="row">
          <button class="icon-button ghost open-menu" type="button" data-action="menu" aria-label="${t('Toggle navigation','Navigation ein- oder ausblenden')}" aria-controls="side-nav" aria-expanded="${state.menu}">${icon('menu')}</button>
          ${brand('mobile-brand')}
          <div class="row crumb desktop-crumb">${icon('grid')}<span>ShortCircuit</span>${icon('chevron')}<strong>${title}</strong>${page === 'link' ? icon('chevron') + t('Details','Details') : ''}</div>
        </div>
        <div class="row tight">
          <button type="button" data-action="domain-picker" class="btn ghost"><span class="row tight">${icon('globe')}<span class="desktop-label">${esc(domains.find(item=>item.primary)?.name || domains[0]?.name)}</span>${icon('down')}</span></button>
          <span class="separator"></span>${iconButton('theme',theme === 'dark' ? 'sun' : 'moon',t('Toggle color theme','Farbschema wechseln'))}
        </div>
      </header>
      <main class="main" id="main" tabindex="-1">${content}</main>
    </div>`;
}
const periods = () => `<select class="select-btn" aria-label="${t('Date range','Zeitraum')}" data-control="range">
  ${[['7',t('Last 7 days','Letzte 7 Tage')],['30',t('Last 30 days','Letzte 30 Tage')],['90',t('Last 90 days','Letzte 90 Tage')]].map(([key,caption])=>`<option value="${key}" ${key===state.range?'selected':''}>${caption}</option>`).join('')}</select>`;
const newLinkCount = () => links.filter(link => link.id > 6).length;
function metrics(items) {
  return `<div class="panel metrics">${items.map(([name,value,helper,glyph])=>`<div class="metric">
    <div class="row tight label">${icon(glyph)}${name}</div><strong>${value}</strong><small>${helper}</small>
  </div>`).join('')}</div>`;
}
const traffic = [28,37,27,40,31,47,42,66,58,31,42,36,52,50,73,57,92,78,54,62,59,74,67,95,56,64,84,75,100,87];
function visitGraph(link) {
  if (link && !link.visits) return `<p class="empty">${t('No visits recorded.','Keine Aufrufe erfasst.')}</p>`;
  const availableDays = link ? 30 : Number(state.range);
  const peak = link ? Math.ceil(link.visits/320)*20 : 400;
  const weights = Array.from({length:availableDays},(_,index)=>{
    const sourceIndex = (index+(link ? link.id*7 : traffic.length-availableDays%traffic.length)) % traffic.length;
    const monthFactor = index < availableDays-60 ? .59 : index < availableDays-30 ? .76 : 1;
    return traffic[sourceIndex]*monthFactor;
  });
  const xy = weights.map((weight,index)=>[index*600/(weights.length-1),160-weight*1.47]);
  const before = xy.map(([x,y],index)=>[x,Math.min(152,36+y*.78+((index*19)%16))]);
  const path = entries => entries.map(([x,y],index)=>(index ? 'L':'M')+x.toFixed(1)+' '+y.toFixed(1)).join(' ');
  const dateLabel = new Intl.DateTimeFormat(t('en-US','de-DE'),{month:'short',day:'numeric',timeZone:'UTC'});
  const dates = Array.from({length:7},(_,index)=>dateLabel.format(new Date(demoDate-86_400_000*Math.round((availableDays-1)*(6-index)/6))));
  return `<section class="panel">
    <div class="panel-head chart-head between"><div><h2>${t('Visits over time','Aufrufe im Zeitverlauf')}</h2><p>${t('Successful redirects by day','Erfolgreiche Weiterleitungen pro Tag')}</p></div>
    <div class="legend"><span><i></i>${t('Current period','Aktueller Zeitraum')}</span><span><i></i>${t('Previous period','Vorheriger Zeitraum')}</span></div></div>
    <div class="chart"><div class="plot">
      ${[0,25,50,75,100].map(percent=>`<span class="plot-label" style="top:${percent}%">${n(peak*(100-percent)/100)}</span>`).join('')}
      <svg viewBox="0 0 600 160" preserveAspectRatio="none" role="img" aria-label="${t('Line chart comparing visits with the previous period','Liniendiagramm mit Aufrufen im Vergleich zum vorherigen Zeitraum')}">
        <defs><linearGradient id="traffic-fill" x1="0" y1="0" x2="0" y2="1" gradientUnits="objectBoundingBox"><stop stop-color="var(--curve)" stop-opacity=".19"/><stop offset="1" stop-color="var(--curve)" stop-opacity=".015"/></linearGradient></defs>
        ${[0,40,80,120,160].map(y=>`<line class="chart-grid" x1="0" x2="600" y1="${y}" y2="${y}"/>`).join('')}
        <path d="${path(xy)} L600 160 L0 160 Z" fill="url(#traffic-fill)"/>
        <path d="${path(before)}" class="chart-old"/><path d="${path(xy)}" class="chart-line"/>
      </svg></div><div class="plot-days">${dates.map(date=>`<span>${date}</span>`).join('')}</div></div>
  </section>`;
}
function healthSummary() {
  const categories = [['Healthy',207,'success'],['Warning',19,'warn'],['Broken',8,'bad'],['Not checked',14,'muted']];
  return `<section class="panel health-box"><div class="panel-head"><h2>${t('Link health','Linkzustand')}</h2><p>${t('Latest check by destination','Letzte Prüfung je Zieladresse')}</p></div>
    <div class="ring-area"><div class="ring" aria-hidden="true"><div><strong>${n(248)}</strong><small>${t('destinations','Zieladressen')}</small></div></div>
    <div class="health-legend">${categories.map(([name,value,style])=>`<div class="between"><div class="row tight"><i class="dot ${style}"></i><span class="soft">${label(name)}</span></div><strong>${n(value)}</strong></div>`).join('')}</div></div>
    <div class="panel-foot"><a class="text-link" href="#health">${t('View health monitor','Linkprüfung ansehen')}${icon('arrow')}</a></div></section>`;
}
const linkCell = link => `<div class="row tight table-link"><span class="link-symbol">${icon(link.symbol)}</span><div class="truncate"><a class="link-title truncate" href="#link/${link.id}">${esc(link.name)}</a><span class="mono soft link-path truncate">${esc(link.domain+'/'+link.slug)}</span></div></div>`;
const tags = link => link.tags.map(tag=>`<span class="tag">${esc(tag)}</span>`).join(' ');
function table(items, options={}) {
  const {select=false, showTags=false} = options;
  if (!items.length) return `<div class="empty">${t('No matching links.','Keine passenden Links.')}</div>`;
  return `<div class="table-wrap"><table><thead><tr>
    ${select ? `<th class="hide-small"><input type="checkbox" data-control="all" aria-label="${t('Select displayed links','Angezeigte Links auswählen')}" ${items.every(link=>state.selected.has(link.id))?'checked':''}></th>` : ''}
    <th>${t('Link','Link')}</th>${showTags ? '<th class="hide-small">Tags</th>' : ''}<th class="hide-small">${t('Status','Status')}</th><th>${t('Visits','Aufrufe')}</th>
    <th class="hide-small">${t('Created','Erstellt')}</th><th><span class="muted">${t('Actions','Aktionen')}</span></th>
    </tr></thead><tbody>${items.map(link=>`<tr>
      ${select ? `<td class="hide-small"><input type="checkbox" data-control="select" data-value="${link.id}" aria-label="${t('Select','Auswählen')} ${esc(link.name)}" ${state.selected.has(link.id)?'checked':''}></td>` : ''}
      <td>${linkCell(link)}</td>${showTags?`<td class="hide-small">${tags(link)}</td>`:''}<td class="hide-small">${badge(linkStatus(link))}</td><td class="mono">${n(link.visits)}</td>
      <td class="hide-small soft">${esc(t(link.day,link.deDay))}</td><td><div class="row actions">${iconButton('copy','copy',t('Copy short link','Kurzlink kopieren'),link.id)}${iconButton('qr','qr',t('Open QR Studio','QR-Studio öffnen'),link.id,'act-last')}</div></td>
    </tr>`).join('')}</tbody></table></div>`;
}
const totalsByRange = {'7':2874,'30':12486,'90':34321};
function overview() {
  const formatter = new Intl.DateTimeFormat(t('en-US','de-DE'),{month:'short',day:'numeric',year:'numeric',timeZone:'UTC'});
  const dateRange = formatter.formatRange(new Date(Date.UTC(2026,9,32-Number(state.range))),new Date(Date.UTC(2026,9,31)));
  return `${pageTitle(t('Overview','Übersicht'),dateRange,`<div class="row tight">${periods()}${primary()}</div>`)}
    <div class="stack">
      ${metrics([
        [t('Total links','Links gesamt'),n(248+newLinkCount()),t('231 active','231 aktiv'),'link'],
        [t('Visits in period','Aufrufe im Zeitraum'),n(totalsByRange[state.range]),`<span class="up">↑ 18.4%</span> ${t('vs. previous','zum Vergleichszeitraum')}`,'cursor'],
        [t('Broken destinations','Defekte Ziele'),n(8),t('19 with warnings','19 mit Warnungen'),'activity'],
        [t('Expiring soon','Laufen bald ab'),n(5),t('In the next 7 days','In den nächsten 7 Tagen'),'clock'],
      ])}
      <div class="board">${visitGraph()}${healthSummary()}</div>
      <section class="panel"><div class="panel-head between"><div><h2>${t('Recent links','Neueste Links')}</h2></div><a class="text-link" href="#links">${t('View all links','Alle Links ansehen')}${icon('arrow')}</a></div>${table(links.slice(0,4))}</section>
    </div>`;
}
function searchLinks() {
  const query = state.search.trim().toLowerCase();
  const filtered = links.filter(link => (!query || [link.name,link.slug,link.domain,link.target,...link.tags].some(value=>value.toLowerCase().includes(query))) &&
    (!state.domain || state.domain===link.domain) && (!state.status || state.status===linkStatus(link)));
  return state.sort === 'visits' ? [...filtered].sort((a,b)=>b.visits-a.visits) : filtered;
}
function options(entries, value) {
  return entries.map(([key,caption])=>`<option value="${esc(key)}" ${key===value?'selected':''}>${esc(caption)}</option>`).join('');
}
function linksView() {
  return `${pageTitle(t('Links','Links'),'',primary())}
    <section class="panel">
      <div class="toolbar">
        <label class="search">${icon('search')}<input aria-label="${t('Search links','Links suchen')}" type="search" data-control="search" placeholder="${t('Search title, slug, or destination…','Titel, Slug oder Zieladresse suchen…')}" value="${esc(state.search)}"></label>
        <select class="select-btn" aria-label="Domain" data-control="domain">${options([['',t('All domains','Alle Domains')],...domains.map(item=>[item.name,item.name])],state.domain)}</select>
        <select class="select-btn" aria-label="Status" data-control="status">${options([['',t('All statuses','Alle Status')],...['Active','Expiring','Expired','Archived','Disabled'].map(key=>[key,label(key)])],state.status)}</select>
        <select class="select-btn" aria-label="${t('Sort','Sortieren')}" data-control="sort">${options([['recent',t('Newest first','Neueste zuerst')],['visits',t('Most visited','Meiste Aufrufe')]],state.sort)}</select>
      </div>
      <div id="link-results">${linkResults()}</div>
    </section>`;
}
function linkResults() {
  const items = searchLinks();
  const selection = state.selected.size;
  const hidden = selection-items.filter(link=>state.selected.has(link.id)).length;
  const counts = selection
    ? `${n(selection)} ${t('selected','ausgewählt')}${hidden ? ` · ${n(hidden)} ${t('outside filter','außerhalb des Filters')}` : ''}`
    : `${n(items.length)} ${t('links in this preview','Links in dieser Vorschau')}`;
  return table(items,{select:true,showTags:true})+`<div class="panel-foot between wrap"><span class="soft" role="status">${counts}</span>
  ${selection ? `<div class="row wrap tight">${button('clear-selection',t('Clear selection','Auswahl aufheben'),{kind:'ghost'})}${button('export',t('Export selection','Auswahl exportieren'),{glyph:'download'})}</div>` : ''}</div>`;
}
function barPanel(title, data, subtitle='') {
  return `<section class="panel"><div class="panel-head"><h2>${title}</h2>${subtitle?`<p>${subtitle}</p>`:''}</div>
    <div class="bar-list">${data.map(([name,percent,value])=>`<div class="bar-item"><div class="between"><span class="soft">${esc(name)}</span><strong>${esc(value)}</strong></div><div class="bar-track"><div class="bar-fill" style="width:${percent}%"></div></div></div>`).join('')}</div></section>`;
}
function analytics() {
  const ratio = totalsByRange[state.range]/12486;
  return `${pageTitle(t('Analytics','Statistiken'),'',periods())}<div class="stack">
    ${metrics([[t('All visits','Alle Aufrufe'),n(totalsByRange[state.range]),t('In selected period','Im gewählten Zeitraum'),'cursor'],
    [t('Human (estimated)','Menschen (geschätzt)'),n(Math.round(9281*ratio)),t('74.3% of visits','74,3 % der Aufrufe'),'activity'],
    [t('Bot visits','Bot-Aufrufe'),n(Math.round(2416*ratio)),t('19.3% of visits','19,3 % der Aufrufe'),'shield'],
    [t('Unknown','Unbekannt'),n(totalsByRange[state.range]-Math.round(9281*ratio)-Math.round(2416*ratio)),t('6.3% of visits','6,3 % der Aufrufe'),'warning']])}
    ${visitGraph()}
    <div class="board board-equal">${barPanel(t('Devices','Geräte'),[[t('Mobile','Mobil'),58,'58%'],['Desktop',35,'35%'],['Tablet',7,'7%']])}
    ${barPanel(t('Referrer domains','Verweisende Domains'),[[t('Direct / unknown','Direkt / unbekannt'),52,'52%'],['search.example',29,'29%'],['docs.homelab.example',19,'19%']])}</div></div>`;
}
function linkDetail(id) {
  const link = links.find(item=>item.id===id);
  if (!link) return pageTitle(t('Link unavailable','Link nicht verfügbar'),'')+'<p>◂ <a href="#links">'+t('Back to links','Zurück zu Links')+'</a></p>';
  const entries = [[t('Short URL','Kurz-URL'),shortcut(link)],[t('Destination','Zieladresse'),link.target],
    [t('Redirect','Weiterleitung'),link.code || '302 Found'],[t('Created','Erstellt'),t(link.day,link.deDay)],
    [t('Expires','Ablaufdatum'),link.expires ? new Intl.DateTimeFormat(t('en-US','de-DE'),{dateStyle:'medium',timeZone:'UTC'}).format(new Date(link.expires)) : t('No expiration','Kein Ablaufdatum')],
    [t('Query forwarding','Query-Weitergabe'),link.forward?t('On','Ein'):t('Off','Aus')]];
  const actions = `<div class="row tight wrap">${button('copy',t('Copy link','Link kopieren'),{glyph:'copy',value:id})}${button('edit',t('Edit link','Link bearbeiten'),{glyph:'edit',kind:'primary',value:id})}</div>`;
  const content = state.detailTab==='traffic' ? visitGraph(link) : state.detailTab==='rules' ?
    `<div class="panel-pad stack"><h2>${t('Default destination','Standardziel')}</h2><p class="mono">${esc(link.target)}</p><p>${t('No conditional rules have been added.','Es wurden keine bedingten Regeln hinzugefügt.')}</p></div>` :
    `<div class="panel-pad"><dl class="info-list">${entries.map(([key,value])=>`<div><dt>${key}</dt><dd>${esc(value)}</dd></div>`).join('')}</dl></div>`;
  return `${pageTitle(esc(link.name),esc(shortcut(link)),actions)}
    <div class="board"><section class="panel"><div class="tabs">${[['details','Details','Details'],['traffic','Visits','Aufrufe'],['rules','Redirect rules','Weiterleitungsregeln']].map(([id,en,de])=>`<button class="tab" type="button" data-action="detail-tab" data-value="${id}" aria-pressed="${state.detailTab===id}">${t(en,de)}</button>`).join('')}</div>${content}</section>
    <div class="stack"><section class="panel panel-pad stack"><div class="between"><h2>${t('Link status','Linkstatus')}</h2>${badge(linkStatus(link))}</div>
    <div class="between"><span class="soft">${t('Health','Erreichbarkeit')}</span>${badge(link.health)}</div><div class="between"><span class="soft">${t('Total visits','Aufrufe gesamt')}</span><strong>${n(link.visits)}</strong></div>
    <div>${tags(link)}</div></section><section class="panel panel-pad between"><h2>QR</h2>${button('qr',t('Open QR Studio','QR-Studio öffnen'),{glyph:'qr',value:id})}</section></div></div>`;
}
function field(caption, control, description='') {
  return `<label class="field"><span>${caption}</span>${control}${description?`<small>${description}</small>`:''}</label>`;
}
function qrSvg(link) {
  const lines = qrFixtures[shortcut(link)]?.[state.strength];
  if (!lines) return '';
  const quiet = Number(state.quiet);
  const width = lines.length+2*quiet;
  const path = lines.flatMap((row,y)=>Array.from(row,(bit,x)=>bit==='1'?'M'+(x+quiet)+' '+(y+quiet)+'h1v1h-1z':'')).join('');
  const color = value => /^#[\da-f]{6}$/i.test(value) ? value : '#192320';
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${width}" shape-rendering="crispEdges" role="img" aria-label="QR code">${state.transparent ? '' : `<rect width="${width}" height="${width}" fill="${color(state.background)}"/>`}<path fill="${color(state.foreground)}" d="${path}"/></svg>`;
}
function luminance(color) {
  const [red,green,blue] = color.slice(1).match(/../g).map(value=>parseInt(value,16)/255).map(channel=>channel <= .04045 ? channel/12.92 : ((channel+.055)/1.055)**2.4);
  return .2126*red+.7152*green+.0722*blue;
}
function qrStudio() {
  const candidates = links.filter(link=>qrFixtures[shortcut(link)]);
  const link = candidates.find(link=>link.id===state.qr) || candidates[0];
  if (!link) return pageTitle(t('QR Studio','QR-Studio'),'') + `<p class="panel empty">${t('No sample QR codes available.','Keine Beispiel-QR-Codes verfügbar.')}</p>`;
  const foreground=luminance(state.foreground), background=luminance(state.background);
  const contrast=(Math.max(foreground,background)+.05)/(Math.min(foreground,background)+.05);
  const message=state.transparent?t('Check the code against its final background.','Code auf dem vorgesehenen Hintergrund prüfen.') :
    contrast<4.5?t('Low contrast. Test with a scan before sharing.','Niedriger Kontrast. Vor dem Teilen scannen.') :
    t('Contrast ratio','Kontrastverhältnis')+' '+contrast.toFixed(1)+':1';
  return `${pageTitle(t('QR Studio','QR-Studio'),'')}
    <div class="board board-equal">
      <section class="panel panel-pad stack"><h2>${t('Code settings','QR-Einstellungen')}</h2>
        ${field(t('Short link','Kurzlink'),`<select data-control="qr-link">${options(candidates.map(item=>[item.id,item.name]),link.id)}</select>`)}
        <div class="fields">${field(t('Foreground','Vordergrund'),`<input type="color" data-control="foreground" value="${state.foreground}">`)}
        ${field(t('Background','Hintergrund'),`<input type="color" data-control="background" value="${state.background}" ${state.transparent?'disabled':''}>`)}
        ${field(t('Error correction','Fehlerkorrektur'),`<select data-control="strength">${options([['L',t('Low (L)','Niedrig (L)')],['M',t('Medium (M)','Mittel (M)')],['Q',t('Quartile (Q)','Quartil (Q)')],['H',t('High (H)','Hoch (H)')]],state.strength)}</select>`)}
        ${field(t('Quiet zone','Ruhezone'),`<select data-control="quiet">${options([[4,t('4 modules','4 Module')],[6,t('6 modules','6 Module')],[8,t('8 modules','8 Module')]],Number(state.quiet))}</select>`)}</div>
        <label class="field-check"><input type="checkbox" data-control="transparent" ${state.transparent?'checked':''}>${t('Transparent background','Transparenter Hintergrund')}</label>
        <div class="row wrap">${button('download-qr',t('Download SVG','SVG herunterladen'),{kind:'primary',glyph:'download',value:link.id})}${button('download-png',t('Download PNG','PNG herunterladen'),{glyph:'download',value:link.id})}</div>
      </section>
      <section class="panel qr-preview"><div><div class="qr-art">${qrSvg(link)}</div><p class="mono">${esc(shortcut(link))}</p><p class="form-note">${message}</p></div></section>
    </div>`;
}
function healthView() {
  const entries=links.filter(link=>!state.filterHealth || link.health===state.filterHealth);
  const statusOrder=['Healthy','Redirected','Warning','Broken','Not checked'];
  const rows=entries.map(link=>`<tr><td>${linkCell(link)}</td><td>${badge(link.health)}</td><td class="hide-small mono">${esc(({'Healthy':'200 · 128 ms','Redirected':'301 → 200','Warning':'429 · Rate limit','Broken':'404 × 3'})[link.health] || '—')}</td>
    <td class="hide-small soft">${esc(state.lastHealth[link.id] || (link.health==='Not checked'?'—':t('Today, 10:24','Heute, 10:24')))}</td><td><div class="row actions">${iconButton('check-health','refresh',t('Simulate check','Prüfung simulieren'),link.id)}</div></td></tr>`);
  return `${pageTitle(t('Health Monitor','Linkprüfung'),t('Sample results. Checks are simulated.','Beispielergebnisse. Die Prüfungen werden simuliert.'),button('check-health',t('Simulate checks','Prüfungen simulieren'),{kind:'primary',glyph:'refresh'}))}
    <div class="stack">${metrics([[t('Healthy','Erreichbar'),n(207),t('Successful response','Erfolgreiche Antwort'),'check'],
     [t('Warnings','Warnungen'),n(19),t('Review recommended','Prüfung empfohlen'),'warning'],
     [t('Broken','Defekt'),n(8),t('Repeated failures','Wiederholte Fehler'),'activity'],
     [t('Not checked','Ungeprüft'),n(14),t('No check recorded','Keine Prüfung erfasst'),'clock']])}
     <section class="panel"><div class="toolbar"><select class="select-btn" aria-label="Status" data-control="health-filter">${options([['',t('All statuses','Alle Status')],...statusOrder.map(status=>[status,label(status)])],state.filterHealth)}</select></div>
      ${rows.length?`<div class="table-wrap"><table><thead><tr><th>${t('Link','Link')}</th><th>Status</th><th class="hide-small">${t('Latest result','Letztes Ergebnis')}</th><th class="hide-small">${t('Checked','Geprüft')}</th><th>${t('Actions','Aktionen')}</th></tr></thead><tbody>${rows.join('')}</tbody></table></div>`:`<div class="empty">${t('No matching destinations.','Keine passenden Ziele.')}</div>`}</section></div>`;
}
function domainsView() {
  return `${pageTitle(t('Domains','Domains'),'',button('domain-add',t('Add domain','Domain hinzufügen'),{kind:'primary',glyph:'plus'}))}
    <section class="panel"><div class="table-wrap"><table><thead><tr><th>Domain</th><th>Status</th><th class="hide-small">${t('Links','Links')}</th><th class="hide-small">${t('404 fallback','404-Ausweichziel')}</th><th>${t('Actions','Aktionen')}</th></tr></thead><tbody>
    ${domains.map(domain=>`<tr><td><div class="row tight"><span class="link-symbol">${icon('globe')}</span><div><strong class="mono">${esc(domain.name)}</strong><div class="soft">${domain.primary?t('Default domain','Standarddomain'):t('Custom domain','Eigene Domain')}</div></div></div></td><td>${badge(domain.status)}</td><td class="hide-small mono">${n(domain.count)}</td><td class="hide-small mono soft">${esc(domain.fallback||'—')}</td><td><div class="row actions">${iconButton('domain-edit','edit',t('Edit domain','Domain bearbeiten'),domain.name)}</div></td></tr>`).join('')}</tbody></table></div></section>`;
}
const settingSections = [
  ['general','settings','General','Allgemein'],['appearance','sun','Appearance','Darstellung'],
  ['privacy','shield','Privacy','Datenschutz'],['access','link','Users & access','Benutzer & Zugriff'],
  ['backup','download','Backup & restore','Backup & Wiederherstellung'],
  ['system','activity','System','System'],
];
function settingsView() {
  let content;
  const timezone=state.settings.timezone || 'Europe/Berlin';
  if (state.setting==='appearance') {
    content=field(t('Theme','Farbschema'),`<select data-control="theme-select">${options([['dark',t('Dark','Dunkel')],['light',t('Light','Hell')],['system','System']],state.theme)}</select>`);
  } else if (state.setting==='privacy') {
    content=`<form class="stack" data-form="privacy">
      <label class="field-check"><input type="checkbox" name="visits" ${state.settings.visits===false?'':'checked'}>${t('Record visits','Aufrufe erfassen')}</label>
      <label class="field-check"><input type="checkbox" name="referrers" ${state.settings.referrers===false?'':'checked'}>${t('Keep referrer domains','Verweisende Domains speichern')}</label>
      ${field(t('Raw visit retention in days','Aufbewahrung einzelner Aufrufe in Tagen'),`<input type="number" name="retention" min="1" max="3650" value="${Number(state.settings.retention||90)}" required>`)}
      <div><button class="btn primary" type="submit">${t('Save changes','Speichern')}</button></div></form>`;
  } else if (state.setting==='access') {
    content=`<dl class="info-list"><div><dt>${t('Administrator','Administrator')}</dt><dd>Alex Devlin</dd></div><div><dt>${t('API keys','API-Schlüssel')}</dt><dd>${t('No demo API keys','Keine Demo-API-Schlüssel')}</dd></div></dl><a class="text-link" href="#login">${t('Open sign-in preview','Anmeldevorschau öffnen')} ${icon('arrow')}</a>`;
  } else if (state.setting==='backup') {
    content=`<dl class="info-list"><div><dt>${t('Schedule','Zeitplan')}</dt><dd>${t('Daily at 03:00','Täglich um 03:00')}</dd></div><div><dt>${t('Last backup','Letztes Backup')}</dt><dd>${t('Today, 03:00','Heute, 03:00')}</dd></div><div><dt>${t('Retention','Aufbewahrung')}</dt><dd>${t('7 daily backups','7 tägliche Backups')}</dd></div></dl>`;
  } else if (state.setting==='system') {
    content=`<dl class="info-list"><div><dt>${t('Environment','Umgebung')}</dt><dd>${t('Local design preview','Lokale Designvorschau')}</dd></div><div><dt>${t('Database','Datenbank')}</dt><dd>${t('Sample data','Beispieldaten')}</dd></div></dl>`;
  } else {
    content=`<form class="stack" data-form="general"><div class="fields">
      ${field(t('Language','Sprache'),`<select name="language">${options([['en','English'],['de','Deutsch']],state.lang)}</select>`)}
      ${field(t('Time zone','Zeitzone'),`<select name="timezone">${options(['Europe/Berlin','UTC','America/New_York'].map(value=>[value,value]),timezone)}</select>`)}
      </div><div><button class="btn primary" type="submit">${t('Save changes','Speichern')}</button></div></form>`;
  }
  const current=settingSections.find(section=>section[0]===state.setting);
  return `${pageTitle(t('Settings','Einstellungen'),'')}
    <div class="settings"><nav class="panel" aria-label="${t('Settings sections','Einstellungsbereiche')}">${settingSections.map(([id,glyph,en,de])=>`<button type="button" class="nav-item" data-action="setting" data-value="${id}" ${id===state.setting?'aria-current="page"':''}>${icon(glyph)}${t(en,de)}</button>`).join('')}</nav>
    <section class="panel settings-body stack"><h2>${t(current[2],current[3])}</h2>${content}</section></div>`;
}
function loginView() {
  return `<main class="login" id="main" tabindex="-1"><div class="panel">
    ${brand('')}<form class="stack" data-form="login"><h1>${t('Sign in','Anmelden')}</h1>
      ${field(t('Username','Benutzername'),'<input type="text" name="user" autocomplete="off" value="demo" required>')}
      ${field(t('Password','Passwort'),'<input type="password" name="password" autocomplete="off">',t('Preview only. Use the prefilled demo user.','Vorschau. Den vorausgefüllten Demo-Benutzer verwenden.'))}
      <button type="submit" class="btn primary">${t('Sign in','Anmelden')}</button>
    </form></div></main>`;
}
function render() {
  syncTheme();
  const {page,id}=route();
  const renderers={overview,links:linksView,link:()=>linkDetail(id),analytics,qr:qrStudio,health:healthView,domains:domainsView,settings:settingsView};
  screen.innerHTML=page==='login'?loginView():shell(page,renderers[page]());
  document.title='ShortCircuit · '+(screen.querySelector('h1')?.textContent || '');
  document.querySelector('.skip').textContent=t('Skip to content','Zum Inhalt springen');
}
function modal(title, body, footer='', form='') {
  dialog.innerHTML=`<form data-form="${form}" class="dialog-form"><div class="between dialog-head"><h2 id="dialog-title">${title}</h2>
    ${iconButton('close','close',t('Close','Schließen'))}</div><div class="dialog-body stack">${body}</div>${footer?'<div class="dialog-foot row">'+footer+'</div>':''}</form>`;
  if (!dialog.open) dialog.showModal();
}
const saveButtons = caption => button('close',t('Cancel','Abbrechen'),{kind:'ghost'})+`<button type="submit" class="btn primary">${caption}</button>`;
function createDialog(existing) {
  const title=existing ? t('Edit link','Link bearbeiten') : t('Create link','Link erstellen');
  const selectedDomain=existing?.domain || domains.find(item=>item.primary)?.name || domains[0]?.name;
  const body=`<input type="hidden" name="id" value="${existing?.id||''}">
    ${field(t('Destination URL','Ziel-URL'),`<input autofocus type="url" name="target" placeholder="https://example.org/path" value="${esc(existing?.target)}" required>`)}
    ${existing ? field(t('Published short URL','Veröffentlichte Kurz-URL'),`<input readonly value="${esc(shortcut(existing))}">`)
      : field(t('Custom slug','Eigener Slug'),`<input name="slug" placeholder="lab/handbook" pattern="[A-Za-z0-9_\\-\\/]+">`,t('Optional. Leave blank to generate a slug.','Optional. Leer lassen für einen generierten Slug.'))}
    ${field('Tags',`<input name="tags" placeholder="homelab, docs" value="${esc(existing?.tags?.join(', '))}">`)}
    <details class="advanced"><summary>${t('Advanced options','Erweiterte Optionen')}</summary>
      <div class="fields">
      ${existing ? '' : field('Domain',`<select name="domain">${options(domains.map(item=>[item.name,item.name]),selectedDomain)}</select>`)}
      ${field(t('Redirect code','Statuscode'),`<select name="code">${options(['302','301','307','308'].map(value=>[value,value]),existing?.code||'302')}</select>`)}
      ${field(t('Title','Titel'),`<input name="title" value="${esc(existing?.name)}">`)}
      ${field(t('Expires','Ablaufdatum'),`<input name="expires" type="date" value="${esc(existing?.expires)}">`)}
      <label class="field-check"><input type="checkbox" name="forward" ${existing?.forward?'checked':''}>${t('Forward query parameters','Query-Parameter weitergeben')}</label>
      <small>${t('301 and 308 may be cached.','301 und 308 können zwischengespeichert werden.')}</small>
      </div></details><p class="form-note bad" id="form-error" role="alert" hidden></p>`;
  modal(title,body,saveButtons(existing?t('Save changes','Speichern'):title),'link');
}
function domainDialog(name='') {
  const current=domains.find(domain=>domain.name===name);
  modal(current?t('Edit domain','Domain bearbeiten'):t('Add domain','Domain hinzufügen'),
    `<input type="hidden" name="previous" value="${esc(name)}">${field(t('Hostname','Hostname'),`<input autofocus name="host" value="${esc(current?.name)}" required ${current?'readonly':''} placeholder="go.homelab.example">`)}
    ${field(t('404 fallback URL','404-Ausweichziel'),`<input type="url" name="fallback" value="${esc(current?.fallback)}" placeholder="https://homelab.example/links">`)}
    <label class="field-check"><input type="checkbox" name="primary" ${current?.primary?'checked disabled':''}>${t('Default domain','Standarddomain')}</label><p class="form-note bad" id="form-error" role="alert" hidden></p>`,
    saveButtons(t('Save domain','Domain speichern')),'domain');
}
let timeout;
function feedback(message) {
  notice.textContent=message;
  clearTimeout(timeout);
  timeout=setTimeout(()=>notice.textContent='',2600);
}
async function copy(link) {
  if (!link) return;
  try {
    await navigator.clipboard.writeText(shortcut(link));
    feedback(t('Short link copied','Kurzlink kopiert'));
  } catch {
    modal(t('Copy short link','Kurzlink kopieren'),field(t('Short URL','Kurz-URL'),`<input readonly value="${esc(shortcut(link))}">`),button('close',t('Close','Schließen')));
    dialog.querySelector('input').select();
  }
}
function download(blob,name) {
  const url=URL.createObjectURL(blob);
  const anchor=document.createElement('a');
  anchor.href=url;
  anchor.download=name;
  anchor.click();
  setTimeout(()=>URL.revokeObjectURL(url),1500);
}
function exportQr(link,png) {
  if (!link) return;
  const svg=qrSvg(link);
  if (!svg) return;
  const slug=link.slug.replaceAll('/','-');
  const blob=new Blob([svg],{type:'image/svg+xml'});
  if (!png) { download(blob,slug+'.svg'); return; }
  const url=URL.createObjectURL(blob);
  const image=new Image();
  const side=qrFixtures[shortcut(link)][state.strength].length+2*Number(state.quiet);
  image.onload=()=>{
    const canvas=document.createElement('canvas');
    canvas.width=canvas.height=side*12;
    const context=canvas.getContext('2d');
    context.imageSmoothingEnabled=false;
    context.drawImage(image,0,0,canvas.width,canvas.height);
    URL.revokeObjectURL(url);
    canvas.toBlob(result=>result && download(result,slug+'.png'),'image/png');
  };
  image.onerror=()=>{ URL.revokeObjectURL(url); feedback(t('PNG export failed','PNG-Export fehlgeschlagen')); };
  image.src=url;
}
function applyTheme(value) {
  state.theme=value;
  localStorage.setItem('sc-theme',value);
  render();
}
function doAction(action,value) {
  const id=Number(value);
  const link=links.find(entry=>entry.id===id);
  const theme=document.documentElement.dataset.theme;
  switch(action) {
    case 'menu':
      state.menu=!state.menu;
      render();
      document.querySelector(state.menu?'.rail nav a':'.open-menu')?.focus();
      break;
    case 'theme':
      applyTheme(theme==='dark'?'light':'dark');
      document.querySelector('[data-action="theme"]')?.focus();
      break;
    case 'create': createDialog(); break;
    case 'edit': if (link) createDialog(link); break;
    case 'close': dialog.close(); break;
    case 'copy': copy(link); break;
    case 'qr':
      if (link && !qrFixtures[shortcut(link)]) { feedback(t('Select a sample link to preview a QR code.','Für die QR-Vorschau einen Beispiellink auswählen.')); break; }
      state.qr=id;
      if (route().page==='qr') render(); else location.hash='qr';
      break;
    case 'profile': location.hash='login'; break;
    case 'detail-tab':
      state.detailTab=value;
      render();
      document.querySelector('.tab[aria-pressed=true]')?.focus();
      break;
    case 'setting':
      state.setting=value;
      render();
      document.querySelector('.settings [aria-current=page]')?.focus();
      break;
    case 'domain-picker':
      modal(t('Public domains','Öffentliche Domains'),domains.map(domain=>`<div class="between"><span class="mono">${esc(domain.name)}</span>${domain.primary?`<span class="tag">${t('Default','Standard')}</span>`:''}</div>`).join(''),button('go-domains',t('Manage domains','Domains verwalten'),{glyph:'globe'}));
      break;
    case 'go-domains': dialog.close(); location.hash='domains'; break;
    case 'domain-add': domainDialog(); break;
    case 'domain-edit': domainDialog(value); break;
    case 'check-health':
      (link?[link]:links).forEach(entry=>{state.lastHealth[entry.id]=t('Just now (simulated)','Gerade eben (simuliert)');});
      render();
      feedback(t('Simulated results shown','Simulierte Ergebnisse angezeigt'));
      break;
    case 'download-qr': exportQr(link,false); break;
    case 'download-png': exportQr(link,true); break;
    case 'clear-selection':
      state.selected.clear();
      refreshLinks();
      document.querySelector('[data-control=search]')?.focus();
      break;
    case 'export':
      download(new Blob([JSON.stringify(links.filter(item=>state.selected.has(item.id)),null,2)],{type:'application/json'}),'shortcircuit-demo-links.json');
      break;
  }
}
document.addEventListener('click',event=>{
  if (event.target.closest('.skip')) {
    event.preventDefault();
    document.querySelector('main')?.focus();
    return;
  }
  const button=event.target.closest('[data-action]');
  if (button) doAction(button.dataset.action,button.dataset.value);
});
function refreshLinks() {
  document.querySelector('#link-results').innerHTML=linkResults();
}
document.addEventListener('input',event=>{
  if (event.target.dataset.control==='search') {
    state.search=event.target.value;
    refreshLinks();
  }
});
document.addEventListener('change',event=>{
  const target=event.target;
  const key=target.dataset.control;
  if (['range','status','sort','domain','foreground','background','strength','quiet'].includes(key)) state[key]=target.value;
  else if (key==='health-filter') state.filterHealth=target.value;
  else if (key==='qr-link') state.qr=Number(target.value);
  else if (key==='transparent') state.transparent=target.checked;
  else if (key==='theme-select') { applyTheme(target.value); return; }
  else if (key==='select' || key==='all') {
    const ids=key==='all' ? searchLinks().map(link=>link.id) : [Number(target.dataset.value)];
    ids.forEach(id=>target.checked?state.selected.add(id):state.selected.delete(id));
    refreshLinks();
    const next=key==='all'?'[data-control="all"]':'[data-control="select"][data-value="'+ids[0]+'"]';
    document.querySelector(next)?.focus();
    return;
  } else return;
  render();
  document.querySelector('[data-control="'+key+'"]')?.focus();
});
function formError(message) {
  const element=dialog.querySelector('#form-error');
  element.hidden=false;
  element.textContent=message;
}
function safeURL(value) {
  try { const url=new URL(value); return ['https:','http:'].includes(url.protocol) && !url.username && !url.password; } catch { return false; }
}
function saveLink(data) {
  const [id,target,slugInput,domainInput,code,name,rawTags,expires,forward]=['id','target','slug','domain','code','title','tags','expires','forward'].map(key=>data.get(key));
  const existing=links.find(link=>link.id===Number(id));
  if (!safeURL(target)) return formError(t('Enter an HTTP or HTTPS destination without embedded credentials.','HTTP- oder HTTPS-Ziel ohne eingebettete Zugangsdaten eingeben.'));
  const slug=existing?.slug || slugInput || crypto.getRandomValues(new Uint32Array(1))[0].toString(36);
  const domain=existing?.domain || domainInput;
  if (slug.split('/').some(segment=>!segment) || links.some(link=>link.id!==Number(id) && link.slug===slug && link.domain===domain)) return formError(t('Use a unique slug with no empty segments.','Eindeutigen Slug ohne leere Segmente verwenden.'));
  const entry={id:Number(id)||Math.max(0,...links.map(link=>link.id))+1,name:name||new URL(target).hostname,slug,domain,target,code,expires,
    forward:!!forward,tags:rawTags.split(',').map(tag=>tag.trim()).filter(Boolean),visits:existing?.visits||0,status:['Archived','Disabled'].includes(existing?.status)?existing.status:'Active',
    health:existing?.health||'Not checked',day:existing?.day||'Today',deDay:existing?.deDay||'Heute',symbol:existing?.symbol||'link'};
  if (existing) links=links.map(link=>link.id===entry.id?entry:link);
  else links.unshift(entry);
  localStorage.setItem('sc-links',JSON.stringify(links));
  dialog.close();
  state.search=''; state.status=''; state.domain='';
  if (route().page==='link' && route().id===entry.id) render(); else location.hash='link/'+entry.id;
  feedback(existing?t('Link updated','Link aktualisiert'):t('Link created. You can now copy it.','Link erstellt. Er kann jetzt kopiert werden.'));
}
function saveDomain(data) {
  const name=String(data.get('host')).trim().toLowerCase();
  const previous=data.get('previous');
  const fallback=data.get('fallback') || '';
  if (!/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+$/.test(name) || (name!==previous && domains.some(domain=>domain.name===name))) return formError(t('Enter a unique hostname without a scheme or port.','Eindeutigen Hostnamen ohne Schema oder Port eingeben.'));
  if (fallback && !safeURL(fallback)) return formError(t('Enter an HTTP or HTTPS fallback.','HTTP- oder HTTPS-Ausweichziel eingeben.'));
  const existing=domains.find(domain=>domain.name===name);
  const primary=!!data.get('primary') || !!existing?.primary;
  if (primary) domains=domains.map(item=>({...item,primary:false}));
  const item={name,primary,fallback,status:existing?.status||'Not checked',count:existing?.count||0};
  if (existing) domains=domains.map(domain=>domain.name===name?item:domain); else domains.push(item);
  localStorage.setItem('sc-domains',JSON.stringify(domains));
  dialog.close();
  render();
  feedback(t('Domain saved','Domain gespeichert'));
}
document.addEventListener('submit',event=>{
  event.preventDefault();
  const form=event.target.dataset.form;
  const data=new FormData(event.target);
  if (form==='link') saveLink(data);
  if (form==='domain') saveDomain(data);
  if (form==='login') location.hash='overview';
  if (form==='general') {
    state.lang=data.get('language');
    state.settings.timezone=data.get('timezone');
    localStorage.setItem('sc-lang',state.lang);
  }
  if (form==='privacy') {
    state.settings={...state.settings,visits:data.has('visits'),referrers:data.has('referrers'),retention:Number(data.get('retention'))};
  }
  if (form==='general' || form==='privacy') {
    localStorage.setItem('sc-settings',JSON.stringify(state.settings));
    render();
    feedback(t('Settings saved','Einstellungen gespeichert'));
  }
});
window.addEventListener('hashchange',()=>{
  state.menu=false;
  state.detailTab='details';
  render();
  document.querySelector('main')?.focus({preventScroll:true});
  scrollTo(0,0);
});
document.addEventListener('keydown',event=>{
  if (event.key==='Escape' && state.menu) doAction('menu');
});
matchMedia('(prefers-color-scheme: light)').addEventListener('change',()=>{if (state.theme==='system') syncTheme();});
state.settings=safeStored('sc-settings',{});
render();
