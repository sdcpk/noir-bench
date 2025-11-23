// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import {Verifier} from "contracts/Verifier.sol";

contract VerifyTest is Test {
    Verifier internal verifier;

    function setUp() public {
        verifier = new Verifier();
    }

    function testVerify() public {
        // Inputs via env (hex without 0x), or defaults to empty
        string memory proofHex = vm.envOr("PROOF_HEX", string(""));
        string memory pubHex = vm.envOr("PUB_INPUTS_HEX", string(""));

        bytes memory proof = vm.parseBytes(string.concat("0x", proofHex));
        bytes memory pubInputs = vm.parseBytes(string.concat("0x", pubHex));

        bool ok = verifier.verify(proof, pubInputs);
        assertTrue(ok, "verify() failed");

        // Compute ABI-encoded calldata length for reference
        bytes memory callData = abi.encodeWithSignature("verify(bytes,bytes)", proof, pubInputs);
        console2.log("CALDATA_BYTES: %s", callData.length);
    }
}



