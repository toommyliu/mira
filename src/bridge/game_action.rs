use super::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameAction {
    AcceptQuest,
    BuyItem,
    EquipItem,
    EquipLoadout,
    GetMapItem,
    LoadEnhShop,
    LoadHairShop,
    LoadShop,
    Rest,
    SellItem,
    Tfer,
    TryQuestComplete,
    UnequipItem,
    WearItem,
    WearLoadout,
}

impl GameAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::AcceptQuest => "acceptQuest",
            Self::BuyItem => "buyItem",
            Self::EquipItem => "equipItem",
            Self::EquipLoadout => "equipLoadout",
            Self::GetMapItem => "getMapItem",
            Self::LoadEnhShop => "loadEnhShop",
            Self::LoadHairShop => "loadHairShop",
            Self::LoadShop => "loadShop",
            Self::Rest => "rest",
            Self::SellItem => "sellItem",
            Self::Tfer => "tfer",
            Self::TryQuestComplete => "tryQuestComplete",
            Self::UnequipItem => "unequipItem",
            Self::WearItem => "wearItem",
            Self::WearLoadout => "wearLoadout",
        }
    }
}

impl From<GameAction> for Value {
    fn from(action: GameAction) -> Self {
        Value::String(action.as_str().to_owned())
    }
}
