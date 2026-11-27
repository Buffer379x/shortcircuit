// App-wide overlays that any view can open.
export const overlays = $state({
  create: false,
  createTarget: '',
  editId: null as number | null,
  qrId: null as number | null,
  palette: false,
  navOpen: false,
  /** Incremented after a link is created, changed, or deleted, so lists can reload. */
  linksVersion: 0,
});

export function openCreate(target = '') {
  overlays.createTarget = target;
  overlays.create = true;
}

export function linksChanged() {
  overlays.linksVersion++;
}
