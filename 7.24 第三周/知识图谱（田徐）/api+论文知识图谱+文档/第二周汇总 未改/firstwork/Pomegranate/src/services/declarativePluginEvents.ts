const DECLARATIVE_TOOLBAR_EVENT = "firstwork:plugin-toolbar-changed";

export function notifyDeclarativePluginToolbarChanged() {
  window.dispatchEvent(new CustomEvent(DECLARATIVE_TOOLBAR_EVENT));
}

export function subscribeDeclarativePluginToolbarChanged(handler: () => void) {
  window.addEventListener(DECLARATIVE_TOOLBAR_EVENT, handler);
  return () => window.removeEventListener(DECLARATIVE_TOOLBAR_EVENT, handler);
}
