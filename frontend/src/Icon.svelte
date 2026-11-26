<script lang="ts">
  const paths = {
    link: 'M10 13a4 4 0 0 0 6 .4l4-4a4 4 0 0 0-5.7-5.6l-2.2 2.2 M14 11a4 4 0 0 0-6-.4l-4 4a4 4 0 0 0 5.7 5.6l2.2-2.2',
    key: 'M15.5 14a5.5 5.5 0 1 0-5.2-7.2A5.5 5.5 0 0 0 10 10l-7 7v4h4v-3h3v-3l1.7-1.7a5.5 5.5 0 0 0 3.8.7z M16 8h.01',
    account: 'M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2 M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0z',
    plus: 'M12 5v14 M5 12h14',
    search: 'M20 20l-4-4 M18 10.5a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0z',
    copy: 'M8 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z M16 5V4a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h1',
    duplicate: 'M9 8h11a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1z M5 16H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h11a1 1 0 0 1 1 1v1 M14.5 11.5v6 M11.5 14.5h6',
    edit: 'M15 5l4 4 M4 20l4.5-1 11-11a2.8 2.8 0 1 0-4-4l-11 11L4 20z',
    trash: 'M3 6h18 M8 6V4h8v2 M5 6l1 14h12l1-14 M10 10v6 M14 10v6',
    download: 'M12 3v12 M7 10l5 5 5-5 M4 16v4h16v-4',
    exit: 'M10 4H5v16h5 M10 12h11 M17 8l4 4-4 4',
    close: 'M6 6l12 12 M18 6 6 18',
    chevron: 'M9 18l6-6-6-6',
    theme: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z M12 3v18',
    globe: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z M3 12h18 M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z',
    toggle: 'M7 7h10a5 5 0 1 1 0 10H7A5 5 0 1 1 7 7z M7 10v4',
    tag: 'M3 3h7l11 11-7 7L3 10V3z M7.5 7.5h.01',
    qr: 'M3 3h6v6H3z M15 3h6v6h-6z M3 15h6v6H3z M15 15h2v2h-2z M21 15v2 M15 21h2 M21 21h.01',
    exchange: 'M3 8h16 M15 4l4 4-4 4 M21 16H5 M9 12l-4 4 4 4',
    analytics: 'M4 19h16 M6 15V9 M12 15V4 M18 15v-4',
    rule: 'M4 6h16 M4 12h16 M4 18h16 M8 4v4 M15 10v4 M10 16v4',
    lock: 'M6 11h12a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2z M8 11V7a4 4 0 0 1 8 0v4 M12 15v2',
    history: 'M3 12a9 9 0 1 1 2.6 6.4 M3 4v8h8 M12 7v5l3 2',
    star: 'M12 3l2.8 5.7 6.3.9-4.6 4.4 1.1 6.3-5.6-3-5.6 3 1.1-6.3L3 9.6l6.2-.9L12 3z',
    archive: 'M4 8h16v13H4z M3 3h18v5H3z M9 12h6',
    unarchive: 'M4 8h16v13H4z M3 3h18v5H3z M12 18v-7 M9 14l3-3 3 3',
  } as const;

  let { name }: { name: keyof typeof paths } = $props();
</script>

<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  <path d={paths[name]} />
</svg>
