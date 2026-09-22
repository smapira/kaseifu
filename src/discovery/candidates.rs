use crate::models::{candidate::CandidateDestination, file_context::FileContext};

pub fn discover(_context: &FileContext) -> Vec<CandidateDestination> {
    // Future implementation:
    //
    // 1. derive search terms from target metadata
    // 2. query Spotlight
    // 3. inspect directories containing similar files
    // 4. aggregate directory frequency
    // 5. score candidate destinations
    //
    // Jev will only receive these validated candidates.

    Vec::new()
}
