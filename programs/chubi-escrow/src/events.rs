use anchor_lang::prelude::*;

#[event]
pub struct MarketCreated {
    pub market_id: String,
    pub authority: Pubkey,
    pub num_sides: u8,
    pub resolution_duration: i64,
    pub allow_withdrawal: bool,
    pub enable_lockout: bool,
}

#[event]
pub struct Deposited {
    pub market_id: String,
    pub maker: Pubkey,
    pub side: u8,
    pub amount: u64,
    pub entry_weight: u64,
    pub nonce: u64,
    pub timestamp: i64,
}

#[event]
pub struct MarketResolved {
    pub market_id: String,
    pub winner: u8,
    pub twd_0: u64,
    pub winner_payout_share: u64,
    pub total_pool: u64,
    pub penalty_pool: u64,
    pub resolver: Pubkey,
}

#[event]
pub struct PayoutClaimed {
    pub market_id: String,
    pub maker: Pubkey,
    pub nonce: u64,
    pub payout: u64,
    pub fee: u64,
    pub is_winner: bool,
}

#[event]
pub struct Withdrawn {
    pub market_id: String,
    pub maker: Pubkey,
    pub nonce: u64,
    pub amount_returned: u64,
    pub penalty_amount: u64,
    pub penalty_bps: u64,
}

#[event]
pub struct Refunded {
    pub market_id: String,
    pub maker: Pubkey,
    pub nonce: u64,
    pub amount: u64,
}

#[event]
pub struct MarketInvalidated {
    pub market_id: String,
}

#[event]
pub struct FeesCollected {
    pub market_id: String,
    pub amount: u64,
    pub recipient: Pubkey,
}
