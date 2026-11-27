// `use:tip={'Text'}` shows a small tooltip on hover and keyboard focus. The text also becomes
// the accessible description, so icon buttons keep their aria-label for the name.
import type { Action } from 'svelte/action';

let el: HTMLDivElement | null = null;

function show(target: HTMLElement, text: string) {
  hide();
  el = document.createElement('div');
  el.className = 'tooltip';
  el.setAttribute('role', 'tooltip');
  el.textContent = text;
  document.body.append(el);
  const r = target.getBoundingClientRect();
  const t = el.getBoundingClientRect();
  let top = r.top - t.height - 8;
  if (top < 8) top = r.bottom + 8;
  const left = Math.min(Math.max(8, r.left + r.width / 2 - t.width / 2), innerWidth - t.width - 8);
  el.style.top = `${top}px`;
  el.style.left = `${left}px`;
}

function hide() {
  el?.remove();
  el = null;
}

export const tip: Action<HTMLElement, string> = (node, text) => {
  let current = text;
  let timer = 0;
  const enter = () => {
    clearTimeout(timer);
    if (current) timer = window.setTimeout(() => show(node, current), 350);
  };
  const focus = () => current && node.matches(':focus-visible') && show(node, current);
  const leave = () => {
    clearTimeout(timer);
    hide();
  };
  node.addEventListener('mouseenter', enter);
  node.addEventListener('mouseleave', leave);
  node.addEventListener('focus', focus);
  node.addEventListener('blur', leave);
  node.addEventListener('click', leave);
  return {
    update(next) {
      current = next;
    },
    destroy() {
      leave();
      node.removeEventListener('mouseenter', enter);
      node.removeEventListener('mouseleave', leave);
      node.removeEventListener('focus', focus);
      node.removeEventListener('blur', leave);
      node.removeEventListener('click', leave);
    },
  };
};
