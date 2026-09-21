NAME = event-driven-hierarchical-rule-composition
OUT = ../../out/paper-latex

.PHONY: all
all:
	mkdir -p $(OUT)
	latexmk -pdf -interaction=nonstopmode -halt-on-error -outdir=$(OUT) $(NAME).tex
	cp $(OUT)/$(NAME).pdf $(NAME).pdf
