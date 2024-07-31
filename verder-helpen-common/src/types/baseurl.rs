use nutype::nutype;
use url::Url;

#[nutype(
    validate(predicate = |u| !u.cannot_be_a_base()),
    derive(FromStr, Debug, Clone, Deserialize, Serialize, Display, AsRef, TryFrom, PartialEq, Eq, Hash)
)]
pub struct BaseUrl(Url);

impl BaseUrl {
    pub fn join(&self, input: &str) -> Url {
        self.as_ref().join(input).unwrap()
    }
}
