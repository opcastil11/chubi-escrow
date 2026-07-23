use anchor_lang::prelude::*;

#[event]
pub struct BetMarketCreated {
    pub market_id: String,
    pub authority: Pubkey,
    pub num_sides: u8,
    pub close_duration: i64,
    /// Wallet earning the 0.5% commission. Pubkey::default() = house market.
    pub creator: Pubkey,
}

#[event]
pub struct BetPlaced {
    pub market_id: String,
    pub bettor: Pubkey,
    pub side: u8,
    pub amount: u64,
    /// Price paid, PRECISION-scaled (the outcome's pool share after the bet).
    pub entry_odds: u64,
    pub nonce: u64,
    pub timestamp: i64,
}

#[event]
pub struct BetMarketResolved {
    pub market_id: String,
    /// 1-indexed winning outcome.
    pub winner: u8,
    pub winner_pool: u64,
    pub total_pool: u64,
    pub resolver: Pubkey,
}

#[event]
pub struct WinningsClaimed {
    pub market_id: String,
    pub bettor: Pubkey,
    pub nonce: u64,
    pub payout: u64,
    /// Protocol fee taken from this bet's profit (2%).
    pub fee: u64,
    /// Creator commission taken from this bet's profit (0.5%, 0 if none).
    pub creator_fee: u64,
}

#[event]
pub struct BetRefunded {
    pub market_id: String,
    pub bettor: Pubkey,
    pub nonce: u64,
    pub amount: u64,
}

#[event]
pub struct BetMarketInvalidated {
    pub market_id: String,
}

#[event]
pub struct BetFeesCollected {
    pub market_id: String,
    pub amount: u64,
    pub recipient: Pubkey,
}

#[event]
pub struct BetCreatorFeesClaimed {
    pub market_id: String,
    pub creator: Pubkey,
    pub amount: u64,
}
