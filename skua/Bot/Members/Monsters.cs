using Skua.Core.Interfaces;
using Skua.Core.Models.Monsters;

namespace Mira.Skua.Bot.Members;

public sealed class Monsters(Func<IScriptInterface> bot)
{
    public List<Monster> CurrentMonsters => bot().Monsters.MapMonsters.FindAll(monster => monster.Cell == bot().Player.Cell);

    public Dictionary<string, List<Monster>> GetCellMonsters() =>
        bot().Monsters.MapMonsters.GroupBy(monster => monster.Cell).ToDictionary(cell => cell.Key, cell => cell.ToList());
}
