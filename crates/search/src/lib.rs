pub mod index;
pub mod query;

pub use index::{MeilisearchIndex, PackageDocument, SearchIndex};
pub use query::{SearchQuery, SearchResults};
