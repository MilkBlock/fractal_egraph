NAME = event-driven-hierarchical-rule-composition
OUT = ../../out/paper-latex
VERSION ?= v0.2.3

.PHONY: all native-egraph
all: native-egraph
	mkdir -p $(OUT)
	latexmk -pdf -interaction=nonstopmode -halt-on-error -outdir=$(OUT) $(NAME).tex
	cp $(OUT)/$(NAME).pdf $(NAME)_$(VERSION).pdf

native-egraph:
	cargo build --offline --manifest-path ../../egglog/Cargo.toml
	python3 ../../tools/paper_egraph_dot.py
