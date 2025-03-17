-- Migration script to add more fields to the requests table

-- Add prover_address and l1_head_block_number columns to requests table
ALTER TABLE requests 
ADD COLUMN prover_address BYTEA,
ADD COLUMN l1_head_block_number BIGINT; 