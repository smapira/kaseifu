pub mod candidate;
pub mod decision;
pub mod file_context;
pub mod similar_file;

// Re-export types at crate root for easier reference
pub use candidate::CandidateDestination;
pub use decision::*;
pub use file_context::{FileContext, NormalizedFileContext};
pub use similar_file::SimilarFile;
