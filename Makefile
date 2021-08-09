SHELL := /bin/bash

# DOWNLOAD_FILE := nginx.zip
DOWNLOAD_FILE := vbox.iso
DOWNLOAD_FILE_BASE_NAME := $(basename $(DOWNLOAD_FILE))

all: install

.PHONY: rlget install test run

rlget: target/debug/rlget

target/debug/rlget:
	cargo build

install:
	cargo install --force --path .

run:
	RUST_BACKTRACE=1 cargo run

test:
	@echo running test
	-rm /tmp/$(DOWNLOAD_FILE)
	RUST_BACKTRACE=1 cargo run -- -t 5 -o test/$(DOWNLOAD_FILE) -d http://localhost/$(DOWNLOAD_FILE) -f /tmp/$(DOWNLOAD_FILE) | tee /tmp/$(DOWNLOAD_FILE_BASE_NAME).log
	ls -l /tmp/$(DOWNLOAD_FILE) test/$(DOWNLOAD_FILE)
	cmp /tmp/$(DOWNLOAD_FILE) test/$(DOWNLOAD_FILE)
	less /tmp/$(DOWNLOAD_FILE_BASE_NAME).log

check:
	@echo $(DOWNLOAD_FILE) $(DOWNLOAD_FILE_BASE_NAME)
