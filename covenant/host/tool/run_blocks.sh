#!/bin/bash

export start_block=1
export end_block=2

for ((block_number=${start_block}; block_number<=${end_block}; block_number++)); do
    echo "Running for block number $block_number"
    RUSTFLAGS="-C target-cpu=native" cargo run -r --bin covenant-host -- --block-number "$block_number" --rpc-url https://archive.goat.network --chain-id 2345
done
