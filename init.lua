-- Hammerspoon configuration for Typr
-- This script monitors keyboard events and sends them to the Deno server

-- Configuration
local serverUrl = "http://localhost:3433"
local isActive = false

-- Function to send events to the Deno server
local function sendEvent(active)
    local json = string.format('{"isActive": %s}', active)
    print("Sending event:", json)  -- Debug logging
    hs.http.post(serverUrl, json, nil, function(status, body, headers)
        if status ~= 200 then
            print("Error sending event:", status, body)
        end
    end)
end

-- Function to send escape event
local function sendEscapeEvent()
    local json = '{"type": "escape"}'
    print("Sending escape event")  -- Debug logging
    hs.http.post(serverUrl, json, nil, function(status, body, headers)
        if status ~= 200 then
            print("Error sending event:", status, body)
        end
    end)
end

-- Monitor keyboard events
local function handleKeyEvent(event)
    local keyCode = event:getKeyCode()
    local flags = event:getFlags()
    local eventType = event:getType()
    
    -- Check for Command + Shift + Space
    if eventType == hs.eventtap.event.types.keyDown and
       keyCode == 49 and  -- Space key
       flags.cmd and
       flags.shift then
        isActive = not isActive
        sendEvent(isActive)
    -- Check for Escape key
    elseif eventType == hs.eventtap.event.types.keyDown and
           keyCode == 53 then  -- Escape key
        sendEscapeEvent()
    end
end

-- Create event tap for keyboard events
local eventTap = hs.eventtap.new({hs.eventtap.event.types.keyDown}, handleKeyEvent)

-- Start monitoring
eventTap:start()

-- Print startup message
print("Typr Hammerspoon config loaded. Monitoring keyboard events...")