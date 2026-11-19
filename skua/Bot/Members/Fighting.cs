using Skua.Core.Interfaces;
using Skua.Core.Models;
using Skua.Core.Models.Monsters;

namespace Mira.Skua.Bot.Members;

public sealed class Kill(Func<IScriptInterface> bot)
{
    private IScriptInterface Bot => bot();

    public void Player(string name)
    {
        Bot.Combat.AttackPlayer(name);
        Bot.Wait.ForMonsterDeath();
    }

    public void Monster(Monster monster) => Monster(monster.MapID, null);

    public void Monster(Monster monster, CancellationToken? token) => Monster(monster.MapID, token);

    public void Monster(string name) => Monster(name, null);

    public void Monster(string name, CancellationToken? token)
    {
        Bot.Wait.ForMonsterSpawn(name);
        if (Bot.Combat.Attack(name))
            UntilDead(token);
    }

    public void Monster(int id) => Monster(id, null);

    public void Monster(int id, CancellationToken? token)
    {
        Bot.Wait.ForMonsterSpawn(id);
        if (Bot.Combat.Attack(id))
            UntilDead(token);
    }

    private void UntilDead(CancellationToken? token)
    {
        Bot.Sleep(Bot.Options.ActionDelay);
        if (token is null)
        {
            Bot.Wait.ForMonsterDeath();
            return;
        }
        Bot.Wait.ForTrue(() => !Bot.Player.Playing || !Bot.Player.HasTarget, null, -1, token.Value);
        Bot.Combat.CancelAutoAttack();
        Bot.Combat.CancelTarget();
    }

    public void ForItem(string name, string item, int quantity, bool tempItem = false) =>
        Farm.For(bot(), () => Monster(name), [item], [quantity], [tempItem]);

    public void ForItem(IEnumerable<string> names, string item, int quantity, bool tempItem = false) =>
        Farm.For(bot(), () => { foreach (var name in names) Monster(name); }, [item], [quantity], [tempItem]);

    public void ForItems(string name, IEnumerable<string> items, IEnumerable<int> quantities, bool tempItems = false) =>
        Farm.For(bot(), () => Monster(name), items, quantities, items.Select(_ => tempItems));

    public void ForItems(string name, IEnumerable<string> items, IEnumerable<int> quantities, IEnumerable<bool> tempItems) =>
        Farm.For(bot(), () => Monster(name), items, quantities, tempItems);

    public void ForItems(IEnumerable<string> names, IEnumerable<string> items, IEnumerable<int> quantities, bool tempItems = false) =>
        Farm.For(bot(), () => { foreach (var name in names) Monster(name); }, items, quantities, items.Select(_ => tempItems));

    public void ForItems(IEnumerable<string> names, IEnumerable<string> items, IEnumerable<int> quantities, IEnumerable<bool> tempItems) =>
        Farm.For(bot(), () => { foreach (var name in names) Monster(name); }, items, quantities, tempItems);
}

public sealed class Hunt(Game game, Func<IScriptInterface> bot)
{
    public void Monster(string name)
    {
        game.Request("combat", "hunt", new { query = name });
        bot().Kill.Monster(name);
    }

    public void Monster(string name, CancellationToken? token) => Monster(name);

    public void Monster(int id)
    {
        game.Request("combat", "hunt", new { query = new { monMapId = id } });
        bot().Kill.Monster(id);
    }

    public void Monster(int id, CancellationToken? token) => Monster(id);

    public void Monster(Monster monster) => Monster(monster.Name);

    public void Monster(Monster monster, CancellationToken? token) => Monster(monster.Name);

    public void ForItem(string name, string item, int quantity, bool tempItem = false) =>
        Farm.For(bot(), () => Monster(name), [item], [quantity], [tempItem]);

    public void ForItem(IEnumerable<string> names, string item, int quantity, bool tempItem = false) =>
        Farm.For(bot(), () => { foreach (var name in names) Monster(name); }, [item], [quantity], [tempItem]);

    public void ForItems(string name, IEnumerable<string> items, IEnumerable<int> quantities, bool tempItems = false) =>
        Farm.For(bot(), () => Monster(name), items, quantities, items.Select(_ => tempItems));

    public void ForItems(string name, IEnumerable<string> items, IEnumerable<int> quantities, IEnumerable<bool> tempItems) =>
        Farm.For(bot(), () => Monster(name), items, quantities, tempItems);

    public void ForItems(IEnumerable<string> names, IEnumerable<string> items, IEnumerable<int> quantities, bool tempItems = false) =>
        Farm.For(bot(), () => { foreach (var name in names) Monster(name); }, items, quantities, items.Select(_ => tempItems));

    public void ForItems(IEnumerable<string> names, IEnumerable<string> items, IEnumerable<int> quantities, IEnumerable<bool> tempItems) =>
        Farm.For(bot(), () => { foreach (var name in names) Monster(name); }, items, quantities, tempItems);

    public void WithPriority(string name, HuntPriorities priority) => Monster(name);

    public void WithPriority(string name, HuntPriorities priority, CancellationToken? token) => Monster(name);

    public void WithPriority(Monster monster, HuntPriorities priority) => Monster(monster);

    public void WithPriority(Monster monster, HuntPriorities priority, CancellationToken? token) => Monster(monster);

    public void WithPriority(int id, HuntPriorities priority) => Monster(id);

    public void WithPriority(int id, HuntPriorities priority, CancellationToken? token) => Monster(id);
}

static class Farm
{
    public static void For(IScriptInterface bot, Action fight, IEnumerable<string> items, IEnumerable<int> quantities, IEnumerable<bool> temporary)
    {
        var wanted = items.Zip(quantities, temporary).ToList();
        bot.Drops.Add(wanted.Where(entry => !entry.Third).Select(entry => entry.First).ToArray());
        bool Done() => wanted.All(entry => entry.Third
            ? bot.TempInv.Contains(entry.First, entry.Second)
            : bot.Inventory.Contains(entry.First, entry.Second));
        while (!bot.ShouldExit && !Done())
        {
            fight();
            foreach (var (item, _, temp) in wanted)
                if (!temp)
                    bot.Drops.Pickup(item);
        }
    }
}
