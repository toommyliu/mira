namespace Mira.Skua.Bot.Members;

public sealed class UltraBossHelper(Game game, Func<global::Skua.Core.Interfaces.IScriptInterface> bot)
{
    public void SetAttacksStopped(bool shouldStop) => bot().Combat.StopAttacking = shouldStop;

    public void EnableCounterAttack() => game.Request("settings", "update", new { antiCounter = true });

    public void DisableCounterAttack() => game.Request("settings", "update", new { antiCounter = false });

    public bool IsCounterAttackEnabled => game.Request("settings", "get")?["antiCounter"]?.GetValue<bool>() ?? false;
}
