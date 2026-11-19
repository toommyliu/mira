using Skua.Core.Models.Items;

namespace Skua.Core.Interfaces;

public interface IJunkService
{
    IReadOnlyList<JunkItemConfig> JunkItems { get; }

    void Load();

    void Save();

    bool IsJunk(int id);

    void SetJunk(IEnumerable<JunkItemConfig> items);

    void SellAllJunk();
}
