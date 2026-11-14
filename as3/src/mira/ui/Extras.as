package mira.ui {
  [BridgeNamespace("extras")]
  public class Extras {
    [BridgeExport]
    public static function setCopyMenusEnabled(enabled:Boolean):void {
      CopyMenu.setEnabled(enabled);
    }

    [BridgeExport]
    public static function setChatRecallEnabled(enabled:Boolean):void {
      ChatRecall.setEnabled(enabled);
    }

    [BridgeExport]
    public static function setBankSearchEnabled(enabled:Boolean):void {
      BankSearch.setEnabled(enabled);
    }

    [BridgeExport]
    public static function setPercentagesEnabled(enabled:Boolean):void {
      Percentages.setEnabled(enabled);
    }
  }
}
