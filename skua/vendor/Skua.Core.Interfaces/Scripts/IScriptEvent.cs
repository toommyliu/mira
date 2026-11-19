using Skua.Core.Models.Items;

namespace Skua.Core.Interfaces;

/// <summary>
/// Represents the method that handles a logout event.
/// </summary>
/// <remarks>
/// Use this delegate to define the signature for event handlers that are invoked when a logout operation
/// occurs. The handler does not receive any event data.
/// </remarks>
public delegate void LogoutEventHandler();

/// <summary>
/// Represents the method that handles a player death event.
/// </summary>
/// <remarks>
/// Use this delegate to define the signature of methods that are called when a player dies. Typically
/// used with events to notify subscribers when a player death occurs.
/// </remarks>
public delegate void PlayerDeathEventHandler();

/// <summary>
/// Represents the method that will handle an event when a monster is killed on a specific map.
/// </summary>
/// <param name="mapId">The identifier of the map where the monster was killed.</param>
public delegate void MonsterKilledEventHandler(int mapId);

/// <summary>
/// Represents the method that handles an event when a quest is accepted.
/// </summary>
/// <param name="questId">The unique identifier of the quest that was accepted.</param>
public delegate void QuestAcceptedEventHandler(int questId);

/// <summary>
/// Represents the method that handles an event when a quest is turned in.
/// </summary>
/// <param name="questId">The unique identifier of the quest that has been turned in.</param>
public delegate void QuestTurnInEventHandler(int questId);

/// <summary>
/// Represents the method that handles an event when a map changes.
/// </summary>
/// <param name="map">The name of the map that has changed. Cannot be null or empty.</param>
public delegate void MapChangedEventHandler(string map);

/// <summary>
/// Represents the method that handles an event when a cell value changes within a map.
/// </summary>
/// <param name="map">The identifier of the map containing the cell that changed.</param>
/// <param name="cell">The identifier of the cell that was changed.</param>
/// <param name="pad">The identifier of the pad associated with the cell change.</param>
public delegate void CellChangedEventHandler(string map, string cell, string pad);

/// <summary>
/// Represents the method that handles a re-login event triggered by the system.
/// </summary>
/// <param name="wasKicked">Indicates whether the re-login was triggered due to being forcibly disconnected. Set to <see langword="true"/> if the
/// user was kicked; otherwise, <see langword="false"/>.</param>
public delegate void ReloginTriggeredEventHandler(bool wasKicked);

/// <summary>
/// Represents the method that will handle an event when an extension packet is received.
/// </summary>
/// <param name="packet">The dynamic object containing the data of the received extension packet.</param>
public delegate void ExtensionPacketEventHandler(dynamic packet);

/// <summary>
/// Represents the method that will handle an event when a packet is received or processed.
/// </summary>
/// <param name="packet">The packet data associated with the event. Cannot be null.</param>
public delegate void PacketEventHandler(string packet);

/// <summary>
/// Represents the method that will handle an event when a user becomes or returns from being away from keyboard (AFK).
/// </summary>
/// <remarks>Use this delegate to define the signature for event handlers that respond to AFK-related events. The
/// handler does not receive any event data.</remarks>
public delegate void AFKEventHandler();

/// <summary>
/// Represents the method that handles an attempt to purchase an item from a shop.
/// </summary>
/// <param name="shopId">The unique identifier of the shop where the purchase is attempted.</param>
/// <param name="itemId">The unique identifier of the item to be purchased.</param>
/// <param name="shopItemId">The unique identifier of the shop-specific item entry being purchased.</param>
public delegate void TryBuyItemHandler(int shopId, int itemId, int shopItemId);

/// <summary>
/// Represents the method that handles a counterattack event.
/// </summary>
/// <param name="faded">A value indicating whether the counterattack was faded. Set to <see langword="true"/> if the counterattack was
/// partially avoided; otherwise, <see langword="false"/>.</param>
public delegate void CounterAttackHandler(bool faded);

/// <summary>
/// Represents the method that handles the event when an item is dropped, providing information about the item, whether
/// it was added to the inventory, and the current quantity remaining.
/// </summary>
/// <param name="item">The item that was dropped.</param>
/// <param name="addedToInv">A value indicating whether the dropped item was added to the inventory. Set to <see langword="true"/> if the item
/// was added; otherwise, <see langword="false"/>.</param>
/// <param name="quantityNow">The current quantity of the item remaining after the drop operation.</param>
public delegate void ItemDroppedHandler(ItemBase item, bool addedToInv, int quantityNow);

/// <summary>
/// Represents the method that handles an event when an item is bought, providing the unique identifier of the purchased
/// item.
/// </summary>
/// <param name="CharItemID">The unique identifier of the item that was bought.</param>
public delegate void ItemBoughtHandler(int CharItemID);

/// <summary>
/// Represents the method that handles an event when an item is sold, providing details about the sale and the item's
/// updated state.
/// </summary>
/// <param name="CharItemID">The unique identifier of the item that was sold.</param>
/// <param name="QuantitySold">The number of units of the item that were sold in the transaction.</param>
/// <param name="CurrentQuantity">The current quantity of the item remaining after the sale.</param>
/// <param name="Cost">The total cost of the items sold, typically in in-game currency units.</param>
/// <param name="IsAC">true if the item is an AdventureCoin (AC) item; otherwise, false.</param>
public delegate void ItemSoldHandler(int CharItemID, int QuantitySold, int CurrentQuantity, int Cost, bool IsAC);

/// <summary>
/// Represents the method that handles the event when an item is added to a bank.
/// </summary>
/// <param name="item">The item that was added to the bank.</param>
/// <param name="quantityNow">The total quantity of the specified item in the bank after the addition.</param>
public delegate void ItemAddedToBankHandler(ItemBase item, int quantityNow);

/// <summary>
/// Represents the method that handles script stopping events and determines whether the script should be stopped based
/// on the provided exception.
/// </summary>
/// <param name="exception">The exception that caused the script to stop, or null if the stop was not due to an exception.</param>
/// <returns>true if the script should be stopped; otherwise, false.</returns>
public delegate bool ScriptStoppingHandler(Exception? exception);

/// <summary>
/// Represents the method that handles an event when a run action is directed to a specific area or zone.
/// </summary>
/// <param name="zone">The name of the zone to which the run action should be directed. Cannot be null or empty.</param>
public delegate void RunToAreaHandler(string zone);

/// <summary>
/// Defines an interface for subscribing to script-related events in the game, such as player actions, quest progress,
/// map changes, and item transactions.
/// </summary>
/// <remarks>
/// Implementations of this interface provide event notifications for various in-game occurrences,
/// allowing scripts to respond to player activity, game state changes, and system events. Event handlers can be
/// attached to react to specific scenarios, such as when a player logs out, completes a quest, or receives an item
/// drop. Some events may require special handling; for example, after a map change, it is recommended to add a delay
/// before invoking map-related methods to ensure the game state is fully updated.
/// </remarks>
public interface IScriptEvent
{
    /// <summary>
    /// Occurs when the player log out of the game.
    /// </summary>
    event LogoutEventHandler Logout;

    /// <summary>
    /// Occurs when the player dies.
    /// </summary>
    event PlayerDeathEventHandler PlayerDeath;

    /// <summary>
    /// Occurs when the player kills a monster.
    /// </summary>
    event MonsterKilledEventHandler MonsterKilled;

    /// <summary>
    /// Occurs when a quest is accepted.
    /// </summary>
    event QuestAcceptedEventHandler QuestAccepted;

    /// <summary>
    /// Occurs when a quest is turned.
    /// </summary>
    event QuestTurnInEventHandler QuestTurnedIn;

    /// <summary>
    /// Occurs when the current map changes.
    /// </summary>
    /// <remarks>Note that the <see cref="MapChanged"/> is fired when you send the join command.<br></br>Before using any <see cref="IScriptMap"/> method in this event, be sure to add a delay.</remarks>
    event MapChangedEventHandler MapChanged;

    /// <summary>
    /// Occurs when the current cell changes.
    /// </summary>
    event CellChangedEventHandler CellChanged;

    /// <summary>
    /// Occurs when auto re-login has been triggered (but the re-login has not been carried out yet).
    /// </summary>
    event ReloginTriggeredEventHandler ReloginTriggered;

    /// <summary>
    /// Occurs when an extension packet is received.
    /// </summary>
    /// <remarks>The extension packet is a <see langword="dynamic"/> object deserialized from JSON.</remarks>
    event ExtensionPacketEventHandler ExtensionPacketReceived;

    /// <summary>
    /// Occurs when the player turns AFK.
    /// </summary>
    event AFKEventHandler PlayerAFK;

    /// <summary>
    /// Occurs when the player attempts to buy an item from a shop.
    /// </summary>
    event TryBuyItemHandler TryBuyItem;

    /// <summary>
    /// Occurs when a counter-attack from an Ultra boss starts/fades.
    /// </summary>
    event CounterAttackHandler CounterAttack;

    /// <summary>
    /// Occurs when an item drops.
    /// </summary>
    event ItemDroppedHandler ItemDropped;

    /// <summary>
    /// Occurs when an item is sold.
    /// </summary>
    event ItemSoldHandler ItemSold;

    /// <summary>
    /// Occurs when an item is bought;
    /// </summary>
    event ItemBoughtHandler ItemBought;

    /// <summary>
    /// Occurs when an accepted item goes to bank.
    /// </summary>
    event ItemAddedToBankHandler ItemAddedToBank;

    /// <summary>
    /// Occurs when the script is finishing, you can place cleanup code here (like reset options and such).
    /// </summary>
    event ScriptStoppingHandler ScriptStopping;

    /// <summary>
    /// Occurs when a safe zone packet is received (Ledgermayne mechanic). </br>
    /// A is the Left zone, B is the Right zone, and "" (empty string) resets the zones.
    /// </summary>
    event RunToAreaHandler RunToArea;

    /// <summary>
    /// Clear all the event handler subscribers.
    /// </summary>
    void ClearHandlers();

    //void OnLogout();
    //void OnCellChanged(string map, string cell, string pad);
    //void OnCounterAttack(bool faded);
    //void OnExtensionPacket(dynamic packet);
    //void OnItemDropped(ItemBase item, bool addedToInv = false, int quantityNow = 0);
    //void OnItemAddedToBank(ItemBase item, int quantityNow);
    //void OnMapChanged(string map);
    //void OnMonsterKilled(int mapId);
    //void OnPlayerAFK();
    //void OnPlayerDeath();
    //void OnQuestAccepted(int questId);
    //void OnQuestTurnIn(int questId);
    //void OnReloginTriggered(bool kicked);
    //void OnRunToArea(string zone);
    //Task<bool?> OnScriptStoppedAsync();
    //void OnTryBuyItem(int shopId, int itemId, int shopItemId);
}