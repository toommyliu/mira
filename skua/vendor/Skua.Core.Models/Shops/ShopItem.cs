using Newtonsoft.Json;
using Skua.Core.Models.Items;

namespace Skua.Core.Models.Shops;

public class ShopItem : ItemBase
{
    /// <summary>
    /// The shop specific item id of this item.
    /// </summary>
    [JsonProperty("ShopItemID")]
    public int ShopItemID { get; set; }

    /// <summary>
    /// The cost of the item.
    /// </summary>
    [JsonProperty("iCost")]
    public int Cost { get; set; }

    /// <summary>
    /// The level of the shop item.
    /// </summary>
    [JsonProperty("iLvl")]
    public int Level { get; set; }

    /// <summary>
    /// The faction needed to buy this item.
    /// </summary>
    [JsonProperty("sFaction")]
    public string Faction { get; set; } = string.Empty;

    /// <summary>
    /// The needed reputation amount to buy this item.
    /// </summary>
    [JsonProperty("iReqRep")]
    public int RequiredReputation { get; set; }

    /// <summary>
    /// Requirements to merge this item (if it is a merge item).
    /// </summary>
    [JsonProperty("turnin")]
    public List<ItemBase> Requirements { get; set; } = new();

    /// <summary>The inventory token used to buy this item.</summary>
    [JsonProperty("TokID")]
    public int TokenID { get; set; }

    /// <summary>The token cost per purchase bundle.</summary>
    [JsonProperty("TokQty")]
    public double TokenQuantity { get; set; }

    /// <summary>The name of the required token.</summary>
    [JsonProperty("TokName")]
    public string TokenName { get; set; } = string.Empty;

    public override bool Equals(object? obj)
    {
        return obj is ShopItem item && item.ID == ID && item.ShopItemID == ShopItemID;
    }

    public override int GetHashCode()
    {
        return HashCode.Combine(ID, ShopItemID);
    }
}