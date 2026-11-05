// Keyboard for a row of tabs: Tab reaches the open one, and the arrow keys, Home
// and End move between them and open the one they land on.

export function tabKeys(node: HTMLElement) {
  function onkeydown(event: KeyboardEvent) {
    const tabs = [...node.querySelectorAll<HTMLElement>('[role="tab"]:not(:disabled)')];
    const at = tabs.indexOf(document.activeElement as HTMLElement);
    if (at < 0 || !tabs.length) return;
    const last = tabs.length - 1;
    const next =
      event.key === 'ArrowRight' ? (at === last ? 0 : at + 1)
      : event.key === 'ArrowLeft' ? (at === 0 ? last : at - 1)
      : event.key === 'Home' ? 0
      : event.key === 'End' ? last
      : -1;
    if (next < 0) return;
    event.preventDefault();
    tabs[next].focus();
    tabs[next].click();
  }
  node.addEventListener('keydown', onkeydown);
  return {
    destroy() {
      node.removeEventListener('keydown', onkeydown);
    },
  };
}
