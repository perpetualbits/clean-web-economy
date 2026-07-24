// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Script} from "forge-std/Script.sol";
import {AcceptAllVerifier} from "../contracts/AcceptAllVerifier.sol";
import {CWEIdentity} from "../contracts/CWEIdentity.sol";
import {CWERegistry} from "../contracts/CWERegistry.sol";
import {CWETiers} from "../contracts/CWETiers.sol";
import {CWEConsumption} from "../contracts/CWEConsumption.sol";
import {CWEPayouts} from "../contracts/CWEPayouts.sol";
import {EarliestRegistrationArbiter} from "../contracts/EarliestRegistrationArbiter.sol";
import {CWEEscrow} from "../contracts/CWEEscrow.sol";
import {CWEJury} from "../contracts/CWEJury.sol";
import {ICWEIdentity} from "../contracts/interfaces/ICWEIdentity.sol";
import {IJury} from "../contracts/interfaces/IJury.sol";

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
    /// @dev The full set of deployed addresses, grouped into a struct so `run()`
    ///      holds one local variable instead of one per contract — the growing
    ///      contract set had pushed the flat-variable version past Solidity's
    ///      stack-depth limit.
    struct Deployed {
        address verifier;
        address identity;
        address registry;
        address tiers;
        address consumption;
        address payouts;
        address arbiter;
        address jury;
        address escrow;
        address owner;
        address aggregator;
    }

    /// @notice Deploy and wire the contracts, then persist their addresses.
    function run() external {
        // The deployer key funds and signs every deployment transaction.
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer = vm.addr(deployerKey);
        Deployed memory d;
        // Owner (fee/allowlist admin) and aggregator (epoch committer) are
        // configurable; on a devnet the deployer plays both roles.
        d.owner = vm.envOr("OWNER", deployer);
        d.aggregator = vm.envOr("AGGREGATOR", deployer);

        vm.startBroadcast(deployerKey);

        // The ZK seam: Phase 1 accepts every proof (decision D2).
        d.verifier = address(new AcceptAllVerifier());
        // The credential registry (H6): the trusted-issuer source of truth that
        // the registry and jury gate their verified-creator/juror checks on,
        // replacing the old per-contract owner allowlists.
        d.identity = address(new CWEIdentity(d.owner));
        // The work registry (payees/splits), owned by `owner`.
        d.registry = address(new CWERegistry(d.owner, ICWEIdentity(d.identity)));
        // The tier table / payment intake, owned by `owner`.
        d.tiers = address(new CWETiers(d.owner));
        // The usage intake, checked by the verifier.
        d.consumption = address(new CWEConsumption(AcceptAllVerifier(d.verifier)));
        // The payout ledger/pool, reading splits from the registry; only the
        // aggregator may commit epochs.
        d.payouts = address(new CWEPayouts(CWERegistry(d.registry), d.aggregator));
        // The Phase 1 arbitration stub: earliest registration wins a dispute.
        d.arbiter = address(new EarliestRegistrationArbiter(CWERegistry(d.registry)));
        // The Phase 2.3 jury: a trusted committee that resolves escrow disputes by
        // majority vote, falling back to the earliest-registration arbiter above
        // on a tie or a silent committee.
        d.jury = address(
            new CWEJury(d.owner, EarliestRegistrationArbiter(d.arbiter), ICWEIdentity(d.identity))
        );
        // The fingerprint-match escrow: holds credit behind a challenge window,
        // consulting the jury on challenges and the registry for splits.
        d.escrow = address(new CWEEscrow(CWERegistry(d.registry), d.aggregator, IJury(d.jury)));

        vm.stopBroadcast();

        // Point subscription revenue at the payout pool. This must be done by the
        // tiers owner; on a devnet that is the deployer, so broadcast as owner.
        if (d.owner == deployer) {
            vm.broadcast(deployerKey);
            CWETiers(d.tiers).setPayoutPool(payable(d.payouts));
        }

        // Authorise the escrow to open disputes on the jury. This must be done by
        // the jury's owner; on a devnet that is the deployer, so broadcast as owner.
        if (d.owner == deployer) {
            vm.broadcast(deployerKey);
            CWEJury(d.jury).setEscrow(d.escrow);
        }

        // Persist the addresses so off-chain tooling (settlement job, extension,
        // demo) can find the contracts without re-parsing broadcast logs.
        _writeDeployments(d);
    }

    /// @dev Serialise the deployment address map and write it to
    ///      `deployments/localhost.json` (path is relative to the project root).
    function _writeDeployments(Deployed memory d) private {
        // Build a single JSON object under a shared key; each `serialize*` call
        // returns the accumulated JSON so the last call holds the full object.
        string memory obj = "deployments";
        vm.serializeAddress(obj, "verifier", d.verifier);
        vm.serializeAddress(obj, "identity", d.identity);
        vm.serializeAddress(obj, "registry", d.registry);
        vm.serializeAddress(obj, "tiers", d.tiers);
        vm.serializeAddress(obj, "consumption", d.consumption);
        vm.serializeAddress(obj, "owner", d.owner);
        vm.serializeAddress(obj, "aggregator", d.aggregator);
        vm.serializeAddress(obj, "payouts", d.payouts);
        vm.serializeAddress(obj, "arbiter", d.arbiter);
        vm.serializeAddress(obj, "jury", d.jury);
        string memory json = vm.serializeAddress(obj, "escrow", d.escrow);

        vm.writeJson(json, "deployments/localhost.json");
    }
}
