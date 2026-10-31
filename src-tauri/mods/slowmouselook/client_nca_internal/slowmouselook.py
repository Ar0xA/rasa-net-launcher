# Slow mouse look fix for the retail client.
#
# The bug (traced in tabula_rasa.exe): during mouse look the 3D view's mouse-move handler
# (client.inputhandlers.HandleCapturedMouseMove) passes every cursor position, in the UI's logical integer units, to
# UserController.MouseLookHandler (FUN_004f5950). That takes the move from the reticle (the root window's centre),
# turns the camera only on an axis that moved MORE than 1 unit (Normal state slot 16, FUN_0054df90), and then always
# snaps the cursor back to the centre (FUN_008a3ae0) - so a move of 1 unit is thrown away. One logical unit is
# screen / logical pixels (1.5 x 1.67 at 2560 x 1600), so slow motion at high resolutions is lost.
#
# The fix: while the cursor is within 1 unit of the centre on both axes, do not call MouseLookHandler. The cursor
# is then not snapped back, so slow motion adds up in its position until it is far enough to turn the camera.
# Left: when one axis passes the threshold, a 1-unit move on the other axis is still dropped by the snap-back.
#
# gameui.Initialize registers the original handler before this module loads (client.main.Initialize runs the
# managers' Initialize, then gameui.SetupInputMappings, which imports client_nca_internal), so this registers its
# own handler on the same widget and event: Python handlers of one event run newest first until one returns
# eHandledStop (FUN_008b4b40 / FUN_008b4ce0), so the original is not called.
import uievent
from client import gameclient
import client.gameui
from client.log import *


def _centre():
    rootWindow = gameclient.GetUIManager().GetRootWindow()
    if rootWindow is None:
        return None
    (w, h) = rootWindow.GetSize()
    return (int(w) / 2, int(h) / 2)


def HandleCapturedMouseMove(widget, event):
    controller = gameclient.GetUserController()
    if controller is None:
        return uievent.eHandledStop
    x = event['LocX']
    y = event['LocY']
    if not client.gameui.InUIMode():
        centre = _centre()
        if centre is not None and abs(x - centre[0]) <= 1 and abs(y - centre[1]) <= 1:
            return uievent.eHandledStop
    controller.MouseLookHandler(x, y)
    return uievent.eHandledStop


def Install():
    worldDisplay = gameclient.GetGameUIManager().GetDisplayWorld()
    if worldDisplay is None:
        TR_LOG_ERROR('slowmouselook: no world display to register on')
        return
    worldDisplay.RegisterPythonMouseEvent(uievent.mouse.eButtonNone, uievent.mouse.eMouseMove, HandleCapturedMouseMove)


Install()
