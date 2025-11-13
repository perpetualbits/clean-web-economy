// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

contract ICWERegistry {
    struct Work { address[] payees; uint96[] splits; uint256 pricePerMin; bytes32 regionRule; }
    mapping(bytes32 => Work) public works;
    event WorkRegistered(bytes32 indexed workId);
    function registerWork(bytes32 workId, address[] calldata payees, uint96[] calldata splits, uint256 pricePerMin, bytes32 regionRule) external {
        require(payees.length == splits.length, "LEN");
        works[workId] = Work(payees, splits, pricePerMin, regionRule);
        emit WorkRegistered(workId);
    }
}
