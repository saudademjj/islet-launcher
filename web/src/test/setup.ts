// Tests run in Node: give them the WebView's storage, and Chinese, the
// launcher's first language, so strings read the same on every machine.
const store = new Map<string, string>();
const storage: Storage = {
  get length() {
    return store.size;
  },
  clear: () => store.clear(),
  getItem: (key) => store.get(key) ?? null,
  key: (index) => [...store.keys()][index] ?? null,
  removeItem: (key) => void store.delete(key),
  setItem: (key, value) => void store.set(key, String(value)),
};
Object.defineProperty(globalThis, "localStorage", { value: storage, configurable: true });
storage.setItem("mc-launcher.language", "zh");
