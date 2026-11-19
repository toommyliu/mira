using System.ComponentModel;
using Skua.Core.Interfaces;
using Skua.Core.Models.Servers;

namespace Mira.Skua.Bot.Members;

public sealed class Servers(Game game, Func<IScriptInterface> bot)
{
    public event PropertyChangedEventHandler? PropertyChanged { add { } remove { } }

    public string LastIP { get; set; } = string.Empty;
    public string LastName { get; set; } = string.Empty;

    public List<Server> CachedServers => bot().Servers.ServerList ?? [];

    public ValueTask<List<Server>> GetServers(bool forceUpdate = false) => ValueTask.FromResult(CachedServers);

    public void Login() => game.Request("auth", "login");

    public void Logout() => game.Request("auth", "logout");

    public bool Relogin(Server server = null!) => Relogin(server?.Name ?? Fallback());

    public bool Relogin(string serverName)
    {
        Logout();
        return Join(serverName);
    }

    public bool Reconnect(string serverName, int loginDelay = 2000)
    {
        Logout();
        bot().Sleep(loginDelay);
        return Join(serverName);
    }

    public bool Reconnect(Server server, int loginDelay = 2000) => Reconnect(server.Name, loginDelay);

    public bool EnsureRelogin(string serverName)
    {
        var tries = Math.Max(1, bot().Options.ReloginTries);
        for (var attempt = 0; attempt < tries && !bot().ShouldExit; attempt++)
        {
            if (Relogin(serverName) && bot().Player.Playing)
                return true;
            bot().Sleep(bot().Options.ReloginTryDelay);
        }
        return bot().Player.Playing;
    }

    public Task<bool> EnsureRelogin(CancellationToken token) => Task.Run(() => EnsureRelogin(Fallback()), token);

    public Task<bool> EnsureRelogin(string serverName, CancellationToken token) => Task.Run(() => EnsureRelogin(serverName), token);

    private bool Join(string serverName)
    {
        if (game.Request("auth", "login")?.GetValue<bool>() != true)
            return false;
        var connected = game.Request("auth", "connect", new { server = serverName });
        if (connected?["ok"]?.GetValue<bool>() != true)
            return false;
        LastName = serverName;
        return true;
    }

    private string Fallback() =>
        !string.IsNullOrEmpty(LastName) ? LastName : bot().Options.ReloginServer ?? "Twilly";
}
