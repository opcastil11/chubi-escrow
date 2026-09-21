use anchor_lang::prelude::*;

#[error_code]
pub enum BetError {
    #[msg("Market ID exceeds 64 characters")]
    MarketIdTooLong,          // 6000
    #[msg("Betting window must be 10 minutes to 7 days")]
    InvalidDuration,          // 6001
    #[msg("Market is not open")]
    MarketNotOpen,            // 6002
    #[msg("Market is not resolved")]
    MarketNotResolved,        // 6003
    #[msg("Invalid outcome index")]
    InvalidSide,              // 6004
    #[msg("Bet below minimum (0.001 SOL)")]
    BetTooSmall,              // 6005
    #[msg("Invalid winner")]
    InvalidWinner,            // 6006
    #[msg("Bet does not belong to this market")]
    BetMarketMismatch,        // 6007
    #[msg("Not your bet")]
    NotYourBet,               // 6008
    #[msg("Nothing to claim — this bet lost")]
    NoPayout,                 // 6009
    #[msg("Already claimed")]
    AlreadyClaimed,           // 6010
    #[msg("Betting is closed for this market")]
    BettingClosed,            // 6011
    #[msg("Betting window has not closed yet")]
    BettingStillOpen,         // 6012
    #[msg("Number of outcomes must be 2-6")]
    InvalidNumSides,          // 6013
    #[msg("Need bets on at least 2 outcomes to resolve")]
    InsufficientSides,        // 6014
    #[msg("Nobody backed the winning outcome — invalidate the market instead")]
    NoWinningBets,            // 6015
    #[msg("Math overflow")]
    MathOverflow,             // 6016
    #[msg("Bet not settled (claim or refund first)")]
    BetNotSettled,            // 6017
    #[msg("Market is not invalid")]
    MarketNotInvalid,         // 6018
    #[msg("Insufficient vault balance")]
    InsufficientVault,        // 6019
    #[msg("No fees to collect")]
    NoFees,                   // 6020
    #[msg("Fee recipient mismatch")]
    FeeRecipientMismatch,     // 6021
    #[msg("Odds moved past your slippage limit before the bet landed")]
    OddsSlippage,             // 6022
    #[msg("Signer is not the creator of this market")]
    NotCreator,               // 6023
    #[msg("No creator fees available to claim")]
    NoCreatorFees,            // 6024
    #[msg("Creator account does not match this market")]
    CreatorMarketMismatch,    // 6025
    #[msg("Dispute window must be 0 to 7 days")]
    InvalidDisputeWindow,     // 6026
    #[msg("Market has no proposed outcome")]
    NotProposed,              // 6027
    #[msg("Dispute window has not elapsed yet")]
    DisputeWindowOpen,        // 6028
    #[msg("Dispute window has already closed")]
    DisputeWindowClosed,      // 6029
    #[msg("Market is not in dispute")]
    NotDisputed,              // 6030
    #[msg("Disputer account does not match the active dispute")]
    DisputerMismatch,         // 6031
}
