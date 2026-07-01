.PHONY: build install test clean dev release package install-compliance-tools compliance-audit sbom release-preflight release-preflight-local commercial-blockers release-smoke live-smoke provider-live-smoke remote-live-smoke verify-commercial-artifacts native-computer-mcp test-native-computer-mcp help

CARGO ?= cargo
INSTALL_DIR ?= $(HOME)/.local/bin
BIN_NAME ?= kiana
EXE_EXT ?= $(if $(filter Windows_NT,$(OS)),.exe,)
TARGET_BIN ?= target/release/$(BIN_NAME)$(EXE_EXT)
INSTALLED_BIN ?= $(INSTALL_DIR)/$(BIN_NAME)$(EXE_EXT)

help:
	@echo "Kiana Code - Makefile 帮助"
	@echo ""
	@echo "使用方法:"
	@echo "  make build       构建 debug 版本"
	@echo "  make release     构建 release 版本"
	@echo "  make release-preflight  检查商业发布前置条件"
	@echo "  make install-compliance-tools  安装 cargo-audit/cargo-deny 到 target/"
	@echo "  make compliance-audit   生成 SBOM 并运行合规审计"
	@echo "  make release-smoke  运行发布前本地 smoke gate"
	@echo "  make live-smoke     运行可选远程真实服务 smoke（需要 token）"
	@echo "  make install     安装到 ~/.local/bin"
	@echo "  make test        运行测试"
	@echo "  make native-computer-mcp       构建电脑控制 MCP native 后端"
	@echo "  make test-native-computer-mcp  测试电脑控制 MCP native 后端"
	@echo "  make clean       清理构建产物"
	@echo "  make dev         开发模式运行"
	@echo ""

build:
	@echo "🔨 构建 debug 版本..."
	$(CARGO) build --bin $(BIN_NAME)

release:
	@echo "🚀 构建 release 版本..."
	$(CARGO) build --release -p kiana-entrypoints --bin $(BIN_NAME)

package:
	bash scripts/package-release.sh

sbom:
	bash scripts/generate-sbom.sh

install-compliance-tools:
	bash scripts/install-compliance-tools.sh

compliance-audit:
	bash scripts/compliance-audit.sh --local-rc

commercial-blockers:
	bash scripts/commercial-release-blockers-report.sh

release-preflight:
	bash scripts/release-preflight.sh

release-preflight-local:
	bash scripts/release-preflight.sh --local-rc

release-smoke:
	@echo "🚦 运行发布前 smoke gate..."
	bash scripts/release-smoke.sh

live-smoke:
	@echo "🌐 运行可选远程真实服务 smoke..."
	bash scripts/provider-live-smoke.sh --required
	bash scripts/remote-live-smoke.sh --required

provider-live-smoke:
	bash scripts/provider-live-smoke.sh --required

remote-live-smoke:
	bash scripts/remote-live-smoke.sh --required

verify-commercial-artifacts:
	bash scripts/verify-commercial-release-artifacts.sh

install: release
	@echo "📦 安装到 $(INSTALL_DIR)..."
	@mkdir -p $(INSTALL_DIR)
	@cp $(TARGET_BIN) $(INSTALL_DIR)/
	@chmod +x $(INSTALLED_BIN)
	@$(INSTALLED_BIN) --version >/dev/null
	@$(INSTALLED_BIN) doctor >/dev/null || true
	@echo "✅ 安装完成: $(INSTALL_DIR)/$(BIN_NAME)"
	@echo ""
	@echo "请确保 $(INSTALL_DIR) 在你的 PATH 中"

test:
	@echo "🧪 运行测试..."
	$(CARGO) test --workspace

native-computer-mcp:
	@echo "🖥️ 构建带电脑控制 MCP native 后端的 kiana 二进制..."
	$(CARGO) build -p kiana-entrypoints --bin $(BIN_NAME) --features native-computer-use
	@echo "运行: ./target/debug/$(BIN_NAME) computer-mcp"

test-native-computer-mcp:
	@echo "🧪 测试电脑控制 MCP native 后端..."
	$(CARGO) test -p kiana-computer-mcp --features native
	$(CARGO) test -p kiana-entrypoints --features native-computer-use cli::tests::computer_mcp_stdio_route_accepts_command_and_legacy_flags

clean:
	@echo "🧹 清理构建产物..."
	$(CARGO) clean
	@rm -f $(INSTALLED_BIN)

dev:
	@echo "🔧 开发模式运行..."
	$(CARGO) run --bin $(BIN_NAME)
