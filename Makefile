.PHONY: all build build-release install app install-app ios ios-sim ios-run test test-all fuzz bench clean release-check fmt lint check setup

# Default target
all: build

# Debug build
build:
	cargo build

# Release build
build-release:
	cargo build --release

# Install stet to ~/.cargo/bin and link the Claude Code skill
install:
	sh scripts/install.sh

# Build Stet.app (macOS) into target/bundle
app: build-release
	scripts/bundle-macos.sh target/release/stet target/bundle

# Put Stet.app in /Applications
install-app: app
	rm -rf /Applications/Stet.app
	ditto target/bundle/Stet.app /Applications/Stet.app
	@echo "Installed /Applications/Stet.app"

# Build Stet.app for iPhone and iPad into target/bundle-ios. Needs Xcode and
# `rustup target add aarch64-apple-ios`; signed if STET_IOS_IDENTITY and
# STET_IOS_PROFILE say how (see scripts/bundle-ios.sh).
IOS_ENV = IPHONEOS_DEPLOYMENT_TARGET=16.0
ios:
	$(IOS_ENV) cargo build --release -p stet-app --target aarch64-apple-ios
	$(IOS_ENV) scripts/bundle-ios.sh target/aarch64-apple-ios/release/stet target/bundle-ios

# The same for the simulator (`rustup target add aarch64-apple-ios-sim`)
ios-sim:
	$(IOS_ENV) cargo build -p stet-app --target aarch64-apple-ios-sim
	$(IOS_ENV) scripts/bundle-ios.sh target/aarch64-apple-ios-sim/debug/stet target/bundle-ios-sim

# Install and start it on the simulator that is running
ios-run: ios-sim
	xcrun simctl install booted target/bundle-ios-sim/Stet.app
	xcrun simctl launch --terminate-running-process booted ai.archastro.stet

# Validate a release candidate. Tagging and pushing stay explicit human actions.
release-check:
	cargo fmt --all -- --check
	cargo clippy --locked --all-targets --all-features -- -D warnings
	RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
	cargo test --locked --all-targets --all-features
	cargo audit

# Run all tests
test:
	cargo test

# Run the complete test suite.
test-all:
	cargo test --all-targets --all-features

# Soak the incremental markdown analysis against full parses.
fuzz:
	STET_FUZZ=50000 cargo test --release -p stet-core incremental_analysis

# Hot-path timings on the example document
bench: build-release
	target/release/stet --bench examples/tour.md

# Clean build artifacts
clean:
	cargo clean

# Format code
fmt:
	cargo fmt

# Lint
lint:
	cargo clippy --all-targets --all-features -- -D warnings

# Check (fast compile check without codegen)
check:
	cargo check

# Setup development environment (install git hooks)
setup:
	cp scripts/pre-commit .git/hooks/pre-commit
	chmod +x .git/hooks/pre-commit
	@echo "Git hooks installed"
