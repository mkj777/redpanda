import { create } from "zustand";
import { api, type Download } from "@/lib/ipc";
import { reportError } from "@/lib/actions";

/** How many entries the list keeps, as the backend does. */
const KEPT = 20;

interface DownloadsState {
  /** Newest first. */
  list: Download[];
  /** Attachments being saved right now, see `attachmentKey`. */
  pending: string[];
  /** Reloads the list and checks which files still exist. */
  refresh: () => Promise<void>;
  /** Saves an attachment to the Downloads folder. */
  download: (messageId: number, index: number) => Promise<void>;
  remove: (id: number) => Promise<void>;
  clear: () => Promise<void>;
}

export const attachmentKey = (messageId: number, index: number) => `${messageId}:${index}`;

/** Bumped by every refresh and every change to the list. A refresh can take a while (it checks
 * each file, maybe on a network share), so its answer only counts while nothing newer happened. */
let version = 0;

export const useDownloads = create<DownloadsState>()((set, get) => ({
  list: [],
  pending: [],
  refresh: async () => {
    const asked = ++version;
    try {
      const list = await api.listDownloads();
      if (asked === version) set({ list });
    } catch (e) {
      reportError(e);
    }
  },
  download: async (messageId, index) => {
    const key = attachmentKey(messageId, index);
    if (get().pending.includes(key)) return;
    set({ pending: [...get().pending, key] });
    try {
      const d = await api.downloadAttachment(messageId, index);
      version++;
      set({ list: [d, ...get().list.filter((x) => x.id !== d.id)].slice(0, KEPT) });
    } catch (e) {
      reportError(e);
    } finally {
      set({ pending: get().pending.filter((k) => k !== key) });
    }
  },
  remove: async (id) => {
    version++;
    set({ list: get().list.filter((d) => d.id !== id) });
    try {
      await api.removeDownload(id);
      version++;
    } catch (e) {
      reportError(e);
      void get().refresh();
    }
  },
  clear: async () => {
    version++;
    set({ list: [] });
    try {
      await api.clearDownloads();
      version++;
    } catch (e) {
      reportError(e);
      void get().refresh();
    }
  },
}));

/** The saved file of an attachment, while it still exists. */
export function savedFile(list: Download[], messageId: number, index: number): Download | undefined {
  return list.find((d) => d.messageId === messageId && d.index === index && d.exists);
}

/** Runs something on a downloaded file. When that fails, for one deleted in the meantime, the
 * list is checked again so it shows the file as gone. */
export async function onFile(run: () => Promise<void>) {
  try {
    await run();
  } catch (e) {
    reportError(e);
    void useDownloads.getState().refresh();
  }
}

/** Makes an element the source of a native file drag out of the window. The browser's own drag
 * is cancelled; the backend then runs the system's drag while the mouse button is still down. */
export function fileDrag(start: () => Promise<void>) {
  return {
    draggable: true,
    onDragStart: (e: React.DragEvent) => {
      e.preventDefault();
      void onFile(start);
    },
  };
}
