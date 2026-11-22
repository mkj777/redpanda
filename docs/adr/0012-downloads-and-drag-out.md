# 0012: Downloads without a dialog, and dragging attachments into other apps

Status: accepted

## Context

Saving an attachment opened the Windows save dialog every time. Getting a PDF invoice into a web app like Sevdesk meant saving it somewhere, then finding it again in the upload dialog of the other app. A webview cannot hand a file to another app by itself: the browser's drag and drop only carries files inside the page.

## Decision

As decided by the user:

- **Download saves straight to the Windows Downloads folder**, like a browser. A name that is taken becomes "name (1).pdf"; a file is never overwritten, since the name is claimed with `create_new`. The save dialog and the remembered folder are gone.
- **A download list** sits behind a button in the title bar, next to minimize. It keeps the last 20 downloads across restarts (`downloads` table, migration 0008). An entry opens the file on click, shows it in Explorer or leaves the list (the file stays). The list checks the files each time it opens and whenever redpanda comes back to the front, and shows "File deleted" for one that is gone.
- **Attachments drag out as files**, from the attachment in an open mail (from a copy in the temp folder, no download needed) and from the download list. The page cancels the browser's own drag on `dragstart` and asks the backend, which runs the system's drag (OLE `DoDragDrop`) on the main thread while the mouse button is still down. Explorer, browsers and other apps take it like a file dragged from Explorer.
- **Library**: the user chose CrabNebula's `tauri-plugin-drag`. redpanda uses its core, the `drag` crate by the same authors, directly from its own two commands instead of the plugin's command. The page then names an attachment or a download entry and never a path, so it cannot drag out any other file, and no plugin permission is needed.
- Drags only ever copy (`DROPEFFECT_COPY`), so dropping a download into another folder on the same drive leaves it in Downloads.

## Consequences

- Attachment names come from the sender. They are cleaned once in `mime::parse::safe_file_name` for every use: no path separators or control characters, no device names like `CON`, and trailing dots and spaces are dropped first, since Windows drops them too and "bill.exe." would otherwise open as a program while passing the check for one.
- Programs (`BLOCKED_EXTENSIONS`) are still only saved, never opened, also from the download list.
- Dragging out is built for Windows only; elsewhere the commands report that.
