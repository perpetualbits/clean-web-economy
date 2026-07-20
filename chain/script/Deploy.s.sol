// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Script} from "forge-std/Script.sol";
import {AcceptAllVerifier} from "../contracts/AcceptAllVerifier.sol";
import {CWERegistry} from "../contracts/CWERegistry.sol";
import {CWETiers} from "../contracts/CWETiers.sol";
import {CWEConsumption} from "../contracts/CWEConsumption.sol";
import {CWEPayouts} from "../contracts/CWEPayouts.sol";

/// @title Deploy
/// @notice Deploys the full Phase 1 contract set and wires them together, then
///         writes the resulting addresses to `deployments/localhost.json`.
/// @dev Run against a local Anvil node, e.g.:
///
///        anvil &
///        PRIVATE_KEY=<anvil key> forge script script/Deploy.s.sol \
///          --rpc-url http://127.0.0.1:8545 --broadcast
///
///      `OWNER` and `AGGREGATOR` may be set in the environment; both default to
///      the deployer address, which is convenient for a local devnet.
contract Deploy is Script {
    /// @notice Deploy and wire the contracts, then persist their addresses.
    function run() external {
        // The deployer key funds and signs every deployment transaction.
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerKey);
        // Owner (fee/allowlist admin) and aggregator (epoch committer) are
        // configurable; on a devnet the deployer plays both roles.
        address owner = vm.envOr("OWNER", deployer);
        address aggregator = vm.envOr("AGGREGATOR", deployer);

        vm.startBroadcast(deployerKey);

        // The ZK seam: Phase 1 accepts every proof (decision D2).
        AcceptAllVerifier verifier = new AcceptAllVerifier();
        // The work registry (payees/splits), owned by `owner`.
        CWERegistry registry = new CWERegistry(owner);
        // The tier table / payment intake, owned by `owner`.
        CWETiers tiers = new CWETiers(owner);
        // The usage intake, checked by the verifier.
        CWEConsumption consumption = new CWEConsumption(verifier);
        // The payout ledger/pool, reading splits from the registry; only the
        // aggregator may commit epochs.
        CWEPayouts payouts = new CWEPayouts(registry, aggregator);

        vm.stopBroadcast();

        // Point subscription revenue at the payout pool. This must be done by the
        // tiers owner; on a devnet that is the deployer, so broadcast as owner.
        if (owner == deployer) {
            vm.broadcast(deployerKey);
            tiers.setPayoutPool(payable(address(payouts)));
        }

        // Persist the addresses so off-chain tooling (settlement job, extension,
        // demo) can find the contracts without re-parsing broadcast logs.
        _writeDeployments(address(verifier), address(registry), address(tiers),
            address(consumption), address(payouts), owner, aggregator);
    }

    /// @dev Serialise the deployment address map and write it to
    ///      `deployments/localhost.json` (path is relative to the project root).
    function _writeDeployments(
        address verifier,
        address registry,
        address tiers,
        address consumption,
        address payouts,
        address owner,
        address aggregator
    ) private {
        // Build a single JSON object under a shared key; each `serialize*` call
        // returns the accumulated JSON so the last call holds the full object.
        string memory obj = "deployments";
        vm.serializeAddress(obj, "verifier", verifier);
        vm.serializeAddress(obj, "registry", registry);
        vm.serializeAddress(obj, "tiers", tiers);
        vm.serializeAddress(obj, "consumption", consumption);
        vm.serializeAddress(obj, "owner", owner);
        vm.serializeAddress(obj, "aggregator", aggregator);
        string memory json = vm.serializeAddress(obj, "payouts", payouts);

        vm.writeJson(json, "deployments/localhost.json");
    }
}
