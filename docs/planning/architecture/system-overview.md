# System overview
Buoy is a Wayland window manager for river 0.4+, speaking
`river-window-management-v1`. It owns all tag/workspace state itself (the
compositor has none), gives every tag a persistent pinned zellij terminal
backdrop, floats everything else above it, and exposes an IPC socket that
drives a fuzzel-based tag-manager picker and a status bar.
