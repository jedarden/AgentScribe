#!/usr/bin/env bash
# Script to run tests with proper BLAS library linking on NixOS

# Get the library paths from nix
BLAS_LIB=$(nix-build --no-out-link '<nixpkgs>' -A blas)/lib
LAPACK_LIB=$(nix-build --no-out-link '<nixpkgs>' -A lapack)/lib

# Set library paths
export LD_LIBRARY_PATH="$BLAS_LIB:$LAPACK_LIB:$LD_LIBRARY_PATH"
export LIBRARY_PATH="$BLAS_LIB:$LAPACK_LIB:$LIBRARY_PATH"
export BLAS_LIB_DIR="$BLAS_LIB"
export LAPACK_LIB_DIR="$LAPACK_LIB"

# Run the tests
cargo test --test parent_session_tests --test subagent_integration_test -- --nocapture
