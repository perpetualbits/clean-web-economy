# Operational targets for the Clean Web Economy devnet and demo.
#
# The `demo`/`test-e2e` targets are fully self-contained (they start and stop
# their own Anvil node) and need only foundry + cargo + jq. The `devnet` target
# brings up a longer-lived environment via Docker for interactive use.

# Local devnet RPC endpoint.
RPC ?= http://127.0.0.1:8545
# Anvil's well-known dev account #0 (address 0xf39F…2266). Devnet only.
PRIVATE_KEY ?= 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80

.PHONY: demo test-e2e hub-demo ownership-demo player-demo devnet deploy stop help

help: ## List the available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  %-12s %s\n", $$1, $$2}'

demo: ## Run the full Phase 1 end-to-end demo (self-contained Anvil)
	bash demo/run_demo.sh

test-e2e: demo ## Alias for `demo`, used by CI

hub-demo: ## Run the Discovery Hub end-to-end demo (self-contained Anvil)
	bash demo/run_hub_demo.sh

ownership-demo: ## Run the ownership/escrow end-to-end demo (self-contained Anvil)
	bash demo/run_ownership_demo.sh

player-demo: ## Run the desktop player-agent end-to-end demo (self-contained Anvil)
	bash demo/run_player_demo.sh

devnet: ## Start an interactive devnet (Anvil + static works server) via Docker
	docker compose -f docker-compose.dev.yml up -d
	@echo "devnet up on $(RPC). Deploy the contracts with: make deploy"

deploy: ## Deploy the contracts to the running devnet
	cd ../chain && PRIVATE_KEY=$(PRIVATE_KEY) forge script script/Deploy.s.sol \
		--rpc-url $(RPC) --broadcast

stop: ## Tear down the Docker devnet
	docker compose -f docker-compose.dev.yml down || true
