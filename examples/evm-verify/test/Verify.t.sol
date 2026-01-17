// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import {HonkVerifier} from "contracts/Verifier.sol";

contract VerifyTest is Test {
    HonkVerifier internal verifier;

    function setUp() public {
        verifier = new HonkVerifier();
    }

    function testVerify() public {
        // Inputs via env (hex without 0x), or defaults to empty
        string memory proofHex = vm.envOr("PROOF_HEX", string(""));
        string memory pubHex = vm.envOr("PUB_INPUTS_HEX", string(""));

        bytes memory proof = vm.parseBytes(string.concat("0x", proofHex));
        bytes memory pubInputBytes = vm.parseBytes(string.concat("0x", pubHex));
        require(pubInputBytes.length % 32 == 0, "public inputs not 32-byte aligned");

        uint256 n = pubInputBytes.length / 32;
        bytes32[] memory pubInputs = new bytes32[](n);
        for (uint256 i = 0; i < n; i++) {
            bytes32 word;
            assembly {
                word := mload(add(add(pubInputBytes, 0x20), mul(i, 0x20)))
            }
            pubInputs[i] = word;
        }

        bool ok = verifier.verify(proof, pubInputs);
        assertTrue(ok, "verify() failed");

        // Compute ABI-encoded calldata length for reference
        bytes memory callData = abi.encodeWithSignature("verify(bytes,bytes32[])", proof, pubInputs);
        console2.log("CALDATA_BYTES: %s", callData.length);
    }
}



