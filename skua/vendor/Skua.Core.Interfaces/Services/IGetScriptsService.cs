using Skua.Core.Models.GitHub;
using Skua.Core.Utils;
using System.ComponentModel;

namespace Skua.Core.Interfaces;

public interface IGetScriptsService : INotifyPropertyChanged
{
    int Downloaded => Scripts.Count(s => s.Downloaded);
    int Outdated => Scripts.Count(s => s.Outdated);
    int Total => Scripts.Count;
    int Missing => Total - Downloaded;
    RangedObservableCollection<ScriptInfo> Scripts { get; }

    ValueTask<List<ScriptInfo>> GetScriptsAsync(IProgress<string>? progress, CancellationToken token);

    Task RefreshScriptsAsync(IProgress<string>? progress, CancellationToken token);

    Task<int> IncrementalUpdateScriptsAsync(IProgress<string>? progress, CancellationToken token);

    Task<long> CheckAdvanceSkillSetsUpdates();

    Task DownloadScriptAsync(ScriptInfo info);

    Task<int> DownloadAllWhereAsync(Func<ScriptInfo, bool> pred);

    Task DeleteScriptAsync(ScriptInfo info);

    Task<bool> UpdateSkillSetsFile();

    Task<bool> UpdateQuestDataFile();

    Task<long> CheckJunkItemsUpdates();

    Task<bool> UpdateJunkItemsFile();
}
