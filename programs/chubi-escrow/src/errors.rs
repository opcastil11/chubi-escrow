use anchor_lang::prelude::*;

#[error_code]
pub enum ChubiError {
    #[msg("Market ID exceeds 64 characters")]
    MarketIdTooLong,          // 6000
    #[msg("Duration must be 10 minutes to 7 days")]
    InvalidDuration,          // 6001
    #[msg("Market is not open")]
    MarketNotOpen,            // 6002
    #[msg("Market is not resolved")]
    MarketNotResolved,        // 6003
    #[msg("Invalid side index")]
    InvalidSide,              // 6004
    #[msg("Deposit below minimum (0.001 SOL)")]
    DepositTooSmall,          // 6005
    #[msg("Invalid winner")]
    InvalidWinner,            // 6006
    #[msg("Position does not belong to this market")]
    PositionMarketMismatch,   // 6007
    #[msg("Not your position")]
    NotYourPosition,          // 6008
    #[msg("No payout available")]
    NoPayout,                 // 6009
    #[msg("Already claimed")]
    AlreadyClaimed,           // 6010
    #[msg("Market has not expired yet")]
    MarketNotExpired,         // 6011
    #[msg("Market is in lockout period")]
    InLockoutPeriod,          // 6012
    #[msg("Withdrawals not allowed on this market")]
    WithdrawalsNotAllowed,    // 6013
    #[msg("Position already withdrawn")]
    AlreadyWithdrawn,         // 6014
    #[msg("Market is not invalid")]
    MarketNotInvalid,         // 6015
    #[msg("Number of sides must be 2-6")]
    InvalidNumSides,          // 6016
    #[msg("Need positions on at least 2 sides to resolve")]
    InsufficientSides,        // 6017
    #[msg("Math overflow")]
    MathOverflow,             // 6018
    #[msg("Position not settled (must claim/refund/withdraw first)")]
    PositionNotSettled,       // 6019
    #[msg("Market already resolved")]
    AlreadyResolved,          // 6020
    #[msg("Insufficient vault balance")]
    InsufficientVault,        // 6021
    #[msg("No fees to collect")]
    NoFees,                   // 6022
    #[msg("Fee recipient mismatch")]
    FeeRecipientMismatch,     // 6023
    #[msg("Market time has passed")]
    MarketExpired,            // 6024
    #[msg("Entry weight dropped below min_weight before the deposit landed")]
    WeightSlippage,           // 6025
}
