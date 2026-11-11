package mira.ui {
  [BridgeNamespace("extras")]
  public class Extras {
    [BridgeExport]
    public static function setCopyMenusEnabled(enabled:Boolean):void {
      CopyMenu.setEnabled(enabled);
    }

    [BridgeExport]
    public static function setChatHistoryEnabled(enabled:Boolean):void {
      ChatHistory.setEnabled(enabled);
    }
  }
}
