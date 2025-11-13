# Typst PDF builds
.PHONY: pdf pdf-leaflet pdf-slides


PDF_OUT := ../pdf


pdf: pdf-leaflet pdf-slides


pdf-leaflet:
	mkdir -p $(PDF_OUT)/leaflets
	typst compile ../leaflets/why_join_onepager.typ $(PDF_OUT)/leaflets/why_join_onepager.pdf


pdf-slides:
	mkdir -p $(PDF_OUT)/promo
	typst compile ../promo/pitch_deck.typ $(PDF_OUT)/promo/pitch_deck.pdf
