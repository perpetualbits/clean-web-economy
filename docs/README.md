## Build Targets — Typst → PDF

* Install Typst: `curl -fsSL https://typst.app/install.sh | sh` (or use your distro package).
* Fonts: the template uses **Inter**; replace or install as needed.
* Build all PDFs: `make -C ops pdf`.
* Art assets (logos) can be added via `#image("path/to/logo.svg")` in Typst.

