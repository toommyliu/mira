namespace Skua.Core.Interfaces;

/// <summary>
/// Defines an interface for managing and querying various gameplay and user interface options, including animation
/// toggles, UI visibility, and advanced option retrieval and assignment.
/// </summary>
/// <remarks>
/// Implementations of this interface provide programmatic access to a range of settings that control
/// visual effects, user interface elements, and gameplay behaviors. These options are typically used to customize the
/// user experience, optimize performance, or automate certain actions. The interface also includes generic methods for
/// retrieving and setting advanced options by name, allowing for flexible extension and integration with option panels.
/// </remarks>
public interface IScriptLite
{
    /// <summary>
    /// Gets or sets a value indicating whether the character select screen is currently active.
    /// </summary>
    bool CharacterSelectScreen { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the custom drops user interface is enabled.
    /// </summary>
    bool CustomDropsUI { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether quest log turn-ins are allowed.
    /// </summary>
    bool QuestLogTurnIns { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether battle pets are enabled.
    /// </summary>
    bool BattlePets { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether static player art is enabled.
    /// </summary>
    bool StaticPlayerArt { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether chat filtering is enabled.
    /// </summary>
    bool ChatFilter { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the chat UI is enabled.
    /// </summary>
    bool ChatUI { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the class actives/auras UI is enabled.
    /// </summary>
    bool AurasUI { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether color sets are enabled.
    /// </summary>
    bool ColorSets { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether draggable drops are enabled.
    /// </summary>
    bool DraggableDrops { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether chat scrolling is disabled.
    /// </summary>
    bool DisableChatScrolling { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether damage numbers are disabled.
    /// </summary>
    bool DisableDamageNumbers { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether sound effects are disabled.
    /// </summary>
    bool DisableSoundFx { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether quest popups are disabled.
    /// </summary>
    bool DisableQuestPopup { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the quest tracker is disabled.
    /// </summary>
    bool DisableQuestTracker { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the quest pinner is enabled.
    /// </summary>
    bool QuestPinner { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether quest progress notifications are enabled.
    /// </summary>
    bool QuestProgressNotifications { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether visual skill cooldowns are enabled.
    /// </summary>
    bool VisualSkillCooldowns { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether ground items are hidden.
    /// </summary>
    bool HideGroundItems { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether healing bubbles are hidden.
    /// </summary>
    bool HideHealingBubbles { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether aura animations are disabled.
    /// </summary>
    bool DisableAuraAnimations { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether player names are hidden.
    /// </summary>
    bool HidePlayerNames { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether debugging features are enabled.
    /// </summary>
    bool Debugger { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether debug packet information is enabled.
    /// </summary>
    bool DebugPacket { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the damage strobe visual effect is disabled.
    /// </summary>
    bool DisableDamageStrobe { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether monster animations are disabled.
    /// </summary>
    bool DisableMonsterAnimation { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the red warning indicator is disabled.
    /// </summary>
    bool DisableRedWarning { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether self-animations are disabled.
    /// </summary>
    bool DisableSelfAnimation { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether skill animations are disabled.
    /// </summary>
    /// <remarks>
    /// Set this property to <see langword="true"/> to prevent skill animations from playing during
    /// skill execution. This can be useful for improving performance or for accessibility purposes.
    /// </remarks>
    bool DisableSkillAnimation { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether weapon animations are disabled.
    /// </summary>
    /// <remarks>
    /// Set this property to <see langword="true"/> to prevent weapon animations from playing. This
    /// can be useful for scenarios where visual effects should be suppressed, such as in performance-critical
    /// situations or when animations are not desired.
    /// </remarks>
    bool DisableWeaponAnimation { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether quantity warnings are enabled.
    /// </summary>
    bool QuantityWarnings { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether monster positions are frozen during gameplay.
    /// </summary>
    bool FreezeMonsterPosition { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether players are hidden from view.
    /// </summary>
    bool HidePlayers { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the user interface should be hidden.
    /// </summary>
    bool HideUI { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether monsters are invisible.
    /// </summary>
    bool InvisibleMonsters { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether a quest should be accepted again after completion.
    /// </summary>
    bool ReacceptQuest { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether name tags are shown.
    /// </summary>
    bool ShowNameTags { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether shadows are shown.
    /// </summary>
    bool ShowShadows { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether guild names are hidden.
    /// </summary>
    bool HideGuildNamesOnly { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether only your own name is hidden.
    /// </summary>
    bool HideYourNameOnly { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether only your ground item is shown.
    /// </summary>
    bool ShowYourGroundItemOnly { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether only your aura animation is shown.
    /// </summary>
    bool ShowYourAuraAnimationOnly { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the monster type is displayed.
    /// </summary>
    bool ShowMonsterType { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether background rendering is smoothed.
    /// </summary>
    bool SmoothBackground { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether dead entities should be automatically untargeted.
    /// </summary>
    bool UntargetDead { get; set; }

    /// <summary>
    /// Gets or sets a value indicating whether the player should avoid self targeting.
    /// </summary>
    bool UntargetSelf { get; set; }

    /// <summary>
    /// Gets the current value of an AQLite option (Advanced Options panel).
    /// </summary>
    /// <typeparam name="T">Type of the value to be retrieved.</typeparam>
    /// <param name="optionName">Name of the option to be retrieved.</param>
    /// <returns>The value <typeparamref name="T"/> of the specified option.</returns>
    T? Get<T>(string optionName);

    /// <summary>
    /// Sets the value of an AQLite option (Advanced Options panel) to the specified value.
    /// </summary>
    /// <typeparam name="T">Type of the value that will be set.</typeparam>
    /// <param name="optionName">Name of the options to be set.</param>
    /// <param name="value">Value that will be set to the specified option.</param>
    void Set<T>(string optionName, T value);
}