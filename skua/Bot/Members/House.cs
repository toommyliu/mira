using Skua.Core.Interfaces;
using Skua.Core.Models.Items;

namespace Mira.Skua.Bot.Members;

public sealed class House(Func<IScriptInterface> bot)
{
    private IScriptHouseInv Self => bot().House;

    public bool ToBank(InventoryItem item)
    {
        bot().Send.Packet($"%xt%zm%bankFromInv%{bot().Map.RoomID}%{item.ID}%{item.CharItemID}%");
        bot().Wait.ForHouseInventoryToBank(item.Name);
        return !Self.Contains(item.Name);
    }

    public bool EnsureToBank(string name) => Self.TryGetItem(name, out InventoryItem? item) && Ensure(item!, () => !Self.Contains(name));

    public bool EnsureToBank(int id) => Self.TryGetItem(id, out InventoryItem? item) && Ensure(item!, () => !Self.Contains(id));

    private bool Ensure(InventoryItem item, Func<bool> done)
    {
        var options = bot().Options;
        for (var attempt = 0; !ToBank(item) && !bot().ShouldExit && bot().Player.Playing && ++attempt < options.MaximumTries;)
            bot().Sleep(options.ActionDelay);
        return done();
    }
}
