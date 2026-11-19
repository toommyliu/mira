using Skua.Core.Models.Items;
using Skua.Core.Utils;

namespace Skua.Core.Interfaces;

/// <summary>
/// Defines methods for transferring inventory items to a bank, including support for bulk operations and conditional
/// transfers by item name, ID, or custom criteria.
/// </summary>
/// <remarks>
/// Implementations of this interface typically provide mechanisms to move items from an inventory to a
/// bank, with options to filter which items are transferred and to ensure transfer attempts are retried according to
/// script options. Inherits inventory checking and limitation capabilities from <see cref="ICheckInventory{T}"/> and
/// <see cref="ILimitedInventory"/>.
/// </remarks>
public interface ICanBank : ICheckInventory<InventoryItem>, ILimitedInventory
{
    /// <summary>
    /// Transfers the item with specified <paramref name="name"/> to the bank.
    /// </summary>
    /// <param name="name">Name of the item to transfer.</param>
    /// <returns><see langword="true"/> if the item was moved to the bank.</returns>
    bool ToBank(string name)
    {
        return TryGetItem(name, out InventoryItem? item) && ToBank(item!);
    }

    /// <summary>
    /// Transfers the item with specified <paramref name="id"/> to the bank.
    /// </summary>
    /// <param name="id">ID of the item to transfer.</param>
    /// <returns><see langword="true"/> if the item was moved to the bank.</returns>
    bool ToBank(int id)
    {
        return TryGetItem(id, out InventoryItem? item) && ToBank(item!);
    }

    /// <summary>
    /// Transfers the item with specified <paramref name="item"/> to the bank.
    /// </summary>
    /// <param name="item">InventoryItem instance to transfer.</param>
    /// <returns><see langword="true"/> if the item was moved to the bank.</returns>
    bool ToBank(InventoryItem item);

    /// <summary>
    /// Transfers the items with specified <paramref name="names"/> to the bank.
    /// </summary>
    /// <param name="names">Names of the items to transfer.</param>
    void ToBank(params string[] names)
    {
        foreach (string t in names)
            ToBank(t);
    }

    /// <summary>
    /// Transfers the items with specified <paramref name="ids"/> to the bank.
    /// </summary>
    /// <param name="ids">IDs of the items to transfer.</param>
    void ToBank(params int[] ids)
    {
        foreach (int t in ids)
            ToBank(t);
    }

    /// <summary>
    /// Ensures the item with specified <paramref name="name"/> will be moved to the bank.
    /// </summary>
    /// <param name="name">Name of the item to transfer.</param>
    /// <remarks>It will try <see cref="IScriptOption.MaximumTries"/> then move on even if the transfer was unsuccessful.</remarks>
    bool EnsureToBank(string name);

    /// <summary>
    /// Ensures the item with specified <paramref name="id"/> will be moved to the bank.
    /// </summary>
    /// <param name="id">ID of the item to transfer.</param>
    /// <remarks>It will try <see cref="IScriptOption.MaximumTries"/> then move on even if the transfer was unsuccessful.</remarks>
    bool EnsureToBank(int id);

    /// <summary>
    /// Ensures the items with specified <paramref name="names"/> will be moved to the bank.
    /// </summary>
    /// <param name="names">Names of the items to transfer.</param>
    /// <remarks>It will try <see cref="IScriptOption.MaximumTries"/> then move on even if the transfer was unsuccessful.</remarks>
    void EnsureToBank(params string[] names)
    {
        foreach (string t in names)
            EnsureToBank(t);
    }

    /// <summary>
    /// Ensures the items with specified <paramref name="ids"/> will be moved to the bank.
    /// </summary>
    /// <param name="ids">IDs of the items to transfer.</param>
    /// <remarks>It will try <see cref="IScriptOption.MaximumTries"/> then move on even if the transfer was unsuccessful.</remarks>
    void EnsureToBank(params int[] ids)
    {
        {
            foreach (int t in ids)
                EnsureToBank(t);
        }
    }

    /// <summary>
    /// Transfers all AC (coin) items that are not equipped to the bank.
    /// </summary>
    /// <remarks>If using from the <see cref="IScriptHouseInv"/>, make sure you have joined your house first.</remarks>
    void BankAllCoinItems()
    {
        Items.Where(i => i is { Coins: true, Equipped: false } && i.Name != "treasure potion").ForEach(i => ToBank(i));
    }

    /// <summary>
    /// Transfers all AC (coin) items that are not equipped and don't have the category listed in <paramref name="filterOut"/> to the bank.
    /// </summary>
    /// <param name="filterOut">Categories of items that will not be banked.</param>
    /// <remarks>If using from the <see cref="IScriptHouseInv"/>, make sure you have joined your house first.</remarks>
    void BankAllCoinItems(ItemCategory[] filterOut)
    {
        Items.Where(i => i is { Coins: true, Equipped: false } && i.Name != "treasure potion" && !filterOut.Contains(i.Category)).ForEach(i => ToBank(i));
    }

    /// <summary>
    /// Transfers all AC (coin) items that are not equipped and don't have the name listed in <paramref name="excludeNames"/> to the bank.
    /// </summary>
    /// <param name="excludeNames">Names of items that will not be banked.</param>
    /// <remarks>If using from the <see cref="IScriptHouseInv"/>, make sure you have joined your house first.</remarks>
    void BankAllCoinItems(params string[] excludeNames)
    {
        Items.Where(i => i is { Coins: true, Equipped: false } && i.Name != "treasure potion" && !excludeNames.Contains(i.Name)).ForEach(i => ToBank(i));
    }

    /// <summary>
    /// Transfers all AC (coin) items that are not equipped and don't have the ID listed in <paramref name="excludeIds"/> to the bank.
    /// </summary>
    /// <param name="excludeIds">IDs of items that will not be banked.</param>
    /// <remarks>If using from the <see cref="IScriptHouseInv"/>, make sure you have joined your house first.</remarks>
    void BankAllCoinItems(params int[] excludeIds)
    {
        Items.Where(i => i is { Coins: true, Equipped: false } && i.Name != "treasure potion" && !excludeIds.Contains(i.ID)).ForEach(i => ToBank(i));
    }

    /// <summary>
    /// Transfers all AC (coin) items that are not equipped and pass (returns <see langword="true"/>) the <paramref name="predicate"/> to the bank.
    /// </summary>
    /// <param name="predicate">Predicate function to apply in the AC items.</param>
    /// <remarks>If using from the <see cref="IScriptHouseInv"/>, make sure you have joined your house first.</remarks>
    void BankAllCoinItems(Predicate<InventoryItem> predicate)
    {
        Items.Where(i => i is { Coins: true, Equipped: false } && i.Name != "treasure potion" && predicate(i)).ForEach(i => ToBank(i));
    }
}